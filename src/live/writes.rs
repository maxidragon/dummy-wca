//! Entering results. Like the WCA, a write is checked, answered with 202,
//! and applied a moment later by a background job, in the order received.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, Weak};
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::time::Instant;

use super::model::Attempt;
use super::{definition, find_round, parameter_missing, recompute, require_scoretake, scoretaking};
use crate::state::{AppState, SharedState};
use crate::v1::{ApiError, ApiResult, Token};

/// One accepted write: a whole attempt list for each competitor.
struct Job {
    competition_id: String,
    round_id: String,
    entries: Vec<(u64, Vec<Attempt>)>,
    apply_at: Instant,
}

/// Writes waiting to be applied, and writes to drop.
#[derive(Default)]
pub struct Queue {
    sender: OnceLock<UnboundedSender<Job>>,
    /// Not on the WCA: how many of each competition's next writes to drop.
    drops: Mutex<HashMap<String, u32>>,
}

impl Queue {
    pub fn drop_next(&self, competition_id: &str) -> u32 {
        let mut drops = self.drops.lock().expect("drop lock is never poisoned");
        let pending = drops.entry(competition_id.to_string()).or_default();
        *pending += 1;
        *pending
    }

    /// Whether this write should be dropped, using up one drop if so.
    fn take_drop(&self, competition_id: &str) -> bool {
        let mut drops = self.drops.lock().expect("drop lock is never poisoned");
        match drops.get_mut(competition_id) {
            Some(pending) if *pending > 0 => {
                *pending -= 1;
                true
            }
            _ => false,
        }
    }

    pub fn reset(&self, competition_id: Option<&str>) {
        let mut drops = self.drops.lock().expect("drop lock is never poisoned");
        match competition_id {
            Some(id) => {
                drops.remove(id);
            }
            None => drops.clear(),
        }
    }
}

fn enqueue(state: &SharedState, job: Job) {
    if state.settings.live_result_delay_ms == 0 {
        apply(state, job);
        return;
    }
    let sender = state.live.queue.sender.get_or_init(|| {
        let (sender, mut receiver) = mpsc::unbounded_channel::<Job>();
        let state: Weak<AppState> = std::sync::Arc::downgrade(state);
        tokio::spawn(async move {
            while let Some(job) = receiver.recv().await {
                tokio::time::sleep_until(job.apply_at).await;
                let Some(state) = state.upgrade() else {
                    return;
                };
                apply(&state, job);
            }
        });
        sender
    });
    sender
        .send(job)
        .expect("the queue's worker runs as long as the server");
}

/// `Live::ResultUpdater.apply_result`, for each entry: replaces the
/// attempts, recomputes best and average, and re-ranks the round.
fn apply(state: &SharedState, job: Job) {
    if state.live.queue.take_drop(&job.competition_id) {
        println!(
            "Dropped a live results write to {} {} on purpose",
            job.competition_id, job.round_id
        );
        return;
    }
    let Some(competition) = state.generated(&job.competition_id) else {
        return;
    };
    let definition = definition(state, &competition);
    let Some(index) = definition.round(&job.round_id) else {
        return;
    };
    let round = &definition.rounds[index];
    let now = Utc::now();
    state.live.with(&competition.id, |results| {
        let Some(round_results) = results.get_mut(&round.wcif_id) else {
            return;
        };
        for (registration_id, attempts) in job.entries {
            // Someone quit or the round was cleared since: nothing to update.
            let Some(position) = round_results
                .iter()
                .position(|result| result.registration_id == registration_id)
            else {
                continue;
            };
            let result = &mut round_results[position];
            let changed = round.set_attempts(result, attempts);
            result.last_attempt_entered_at = now;
            let locked = result.locked;
            if changed {
                recompute(&definition, index, round_results, locked);
            }
        }
    });
}

/// `attempts: [{value, attempt_number}]`, or `None` if malformed.
fn attempts(value: &Value) -> Option<Vec<Attempt>> {
    value
        .as_array()?
        .iter()
        .map(|attempt| {
            Some(Attempt {
                value: integer(&attempt["value"])?,
                attempt_number: integer(&attempt["attempt_number"])?,
            })
        })
        .collect()
}

/// A number, or a string of one, as Rails params come.
fn integer(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn unprocessable(status: &str, registration_id: Option<u64>) -> ApiError {
    let mut body = json!({ "status": status });
    if let Some(registration_id) = registration_id {
        body["registration_id"] = json!(registration_id);
    }
    ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        body,
    }
}

const ZERO_VALUE: &str = "Values cannot be 0, please omit them instead";

fn accepted() -> ApiResult {
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "ok" }))).into_response())
}

/// Checks a batch against the round's results as they stand. One bad entry
/// rejects the whole batch.
fn check_entries(
    state: &SharedState,
    competition_id: &str,
    round_id: &str,
    entries: &[(u64, Vec<Attempt>)],
    batch: bool,
) -> Result<(), ApiError> {
    state.live.with(competition_id, |results| {
        let round_results = results.get(round_id).map(Vec::as_slice).unwrap_or_default();
        if batch && round_results.is_empty() {
            return Err(unprocessable("round is not open", None));
        }
        for (registration_id, attempts) in entries {
            let shown_id = batch.then_some(*registration_id);
            if round_results
                .iter()
                .all(|result| result.registration_id != *registration_id)
            {
                if round_results.is_empty() {
                    return Err(unprocessable("round is not open", None));
                }
                return Err(unprocessable("user is not part of this round", shown_id));
            }
            if attempts.iter().any(|attempt| attempt.value == 0) {
                return Err(unprocessable(ZERO_VALUE, shown_id));
            }
        }
        Ok(())
    })
}

/// `POST`/`PATCH /live/rounds/:round_id` with `registration_id` and
/// `attempts`: replaces that competitor's attempts.
pub async fn add_or_update_result(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    token: Token,
    body: Option<Json<Value>>,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let body = body.map(|Json(body)| body).unwrap_or_default();
    let attempts = attempts(&body["attempts"]).ok_or_else(parameter_missing)?;
    let registration_id = integer(&body["registration_id"])
        .and_then(|id| u64::try_from(id).ok())
        .ok_or_else(parameter_missing)?;
    let index = find_round(&ctx.definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let round_id = ctx.definition.rounds[index].wcif_id.clone();
    let entries = vec![(registration_id, attempts)];
    check_entries(&state, &ctx.competition.id, &round_id, &entries, false)?;
    enqueue(&state, job(&state, &ctx.competition.id, round_id, entries));
    accepted()
}

/// `POST /live/rounds/:round_id/batch` with `results: [{registration_id,
/// attempts}]`: all of them, or none if any is wrong.
pub async fn batch_add_or_update_results(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    token: Token,
    body: Option<Json<Value>>,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let body = body.map(|Json(body)| body).unwrap_or_default();
    let entries = body["results"]
        .as_array()
        .ok_or_else(parameter_missing)?
        .iter()
        .map(|entry| {
            let registration_id =
                integer(&entry["registration_id"]).and_then(|id| u64::try_from(id).ok())?;
            Some((registration_id, attempts(&entry["attempts"])?))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(parameter_missing)?;
    let index = find_round(&ctx.definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let round_id = ctx.definition.rounds[index].wcif_id.clone();
    check_entries(&state, &ctx.competition.id, &round_id, &entries, true)?;
    enqueue(&state, job(&state, &ctx.competition.id, round_id, entries));
    accepted()
}

fn job(
    state: &SharedState,
    competition_id: &str,
    round_id: String,
    entries: Vec<(u64, Vec<Attempt>)>,
) -> Job {
    Job {
        competition_id: competition_id.to_string(),
        round_id,
        entries,
        apply_at: Instant::now() + Duration::from_millis(state.settings.live_result_delay_ms),
    }
}

/// Not a WCA endpoint: the competition's next accepted write is answered
/// 202 as usual but never applied, to test what a client does when a write
/// is lost. Each call drops one more.
pub async fn drop_next_write(
    State(state): State<SharedState>,
    Path(competition_id): Path<String>,
) -> ApiResult {
    let competition = crate::v1::competition(&state, &competition_id)?;
    let pending = state.live.queue.drop_next(&competition.id);
    Ok(Json(json!({ "status": "ok", "pendingDrops": pending })).into_response())
}
