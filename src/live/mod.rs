//! The WCA's integrated live results API, under
//! `/api/v1/competitions/:id/live`. Results are kept in memory, like patches.

pub mod model;
pub mod page;
pub mod writes;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod writes_tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, RawQuery, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};

use crate::auth::Caller;
use crate::generator::{
    names,
    wcif::{self, Version},
    Competition,
};
use crate::state::SharedState;
use crate::v1::{self, can_manage, require_user, ApiError, ApiResult, Token};
use model::{Definition, LiveResult, Rounds, Source, State as RoundState};

/// Live results by competition, then by WCIF round id.
#[derive(Default)]
pub struct Store {
    competitions: Mutex<HashMap<String, HashMap<String, Vec<LiveResult>>>>,
    next_id: AtomicU64,
    pub queue: writes::Queue,
}

pub type RoundResults = HashMap<String, Vec<LiveResult>>;

impl Store {
    /// Runs `f` with one competition's results, under the store's lock.
    pub fn with<R>(&self, competition_id: &str, f: impl FnOnce(&mut RoundResults) -> R) -> R {
        let mut competitions = self
            .competitions
            .lock()
            .expect("live results lock is never poisoned");
        f(competitions.entry(competition_id.to_string()).or_default())
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Drops one competition's results, or all. Returns whether there were any.
    pub fn reset(&self, competition_id: Option<&str>) -> Vec<String> {
        self.queue.reset(competition_id);
        let mut competitions = self
            .competitions
            .lock()
            .expect("live results lock is never poisoned");
        let had: Vec<String> = competitions
            .iter()
            .filter(|(id, rounds)| {
                competition_id.is_none_or(|wanted| wanted == *id)
                    && rounds.values().any(|results| !results.is_empty())
            })
            .map(|(id, _)| id.clone())
            .collect();
        match competition_id {
            Some(id) => {
                competitions.remove(id);
            }
            None => competitions.clear(),
        }
        had
    }
}

/// Each round's results, by round index.
pub fn rounds<'a>(definition: &'a Definition, results: &'a RoundResults) -> Rounds<'a> {
    Rounds {
        definition,
        results: definition
            .rounds
            .iter()
            .map(|round| results.get(&round.wcif_id).map_or(&[][..], Vec::as_slice))
            .collect(),
    }
}

/// The rounds and registrations, from the WCIF with patches applied.
pub fn definition(state: &SharedState, competition: &Competition) -> Definition {
    let mut document = wcif::wcif(competition, Version::V2);
    state.apply_patches(&competition.id, &mut document);
    Definition::from_wcif(&document)
}

/// `require_scoretaking_internal`.
fn require_internal(competition: &Competition) -> Result<(), ApiError> {
    if competition.scoretaking_software == "internal" {
        Ok(())
    } else {
        Err(ApiError::not_permitted(
            "Scoretaking software needs to be set to Internal",
        ))
    }
}

fn user_id_of(state: &SharedState, wca_id: &str) -> Option<u64> {
    if let Some(account) = names::account(wca_id) {
        return Some(account.user_id);
    }
    state.competitions().iter().find_map(|competition| {
        competition
            .person_by_wca_id(wca_id)
            .map(|person| person.wca_user_id)
    })
}

/// `require_scoretake!`: managers and scoretakers.
pub fn require_scoretake(
    state: &SharedState,
    competition: &Competition,
    caller: &Caller,
) -> Result<(), ApiError> {
    let is_scoretaker = user_id_of(state, caller.wca_id)
        .is_some_and(|user_id| state.scoretakers(&competition.id).contains(&user_id));
    if can_manage(state, competition, caller.wca_id) || is_scoretaker {
        Ok(())
    } else {
        Err(ApiError::not_permitted("Score taking privileges required"))
    }
}

pub fn round_not_found(round_id: &str) -> ApiError {
    ApiError::with_data(
        StatusCode::NOT_FOUND,
        format!("Couldn't find Round with wcif_id={}", round_id),
        json!({ "model": "Round", "id": null }),
    )
}

pub fn find_round(definition: &Definition, round_id: &str) -> Result<usize, ApiError> {
    definition
        .round(round_id)
        .ok_or_else(|| round_not_found(round_id))
}

fn bad_request(status: &str) -> ApiError {
    ApiError {
        status: StatusCode::BAD_REQUEST,
        body: json!({ "status": status }),
    }
}

/// `ActionController::ParameterMissing`.
pub fn parameter_missing() -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, -4000)
}

/// The competition, for a signed-in user, with internal scoretaking.
pub struct Scoretaking<'a> {
    pub caller: Caller<'a>,
    pub competition: Arc<Competition>,
    pub definition: Definition,
}

pub fn scoretaking<'a>(
    state: &SharedState,
    competition_id: &str,
    token: &'a Token,
) -> Result<Scoretaking<'a>, ApiError> {
    let caller = require_user(token)?;
    let competition = v1::competition(state, competition_id)?;
    require_internal(&competition)?;
    let definition = definition(state, &competition);
    Ok(Scoretaking {
        caller,
        competition,
        definition,
    })
}

/// Recomputes a round's positions and who proceeds.
pub fn recompute(
    definition: &Definition,
    index: usize,
    results: &mut [LiveResult],
    skip_advancing: bool,
) {
    let next_condition =
        definition
            .next_round(index)
            .and_then(|next| match definition.rounds[next].source {
                Source::Round(_, condition) => condition,
                Source::Registrations => None,
            });
    definition.rounds[index].recompute(next_condition, results, skip_advancing);
}

/// `GET /live/rounds/:round_id`: public, like on the WCA.
pub async fn round_results(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
) -> ApiResult {
    let competition = v1::competition(&state, &competition_id)?;
    let definition = definition(&state, &competition);
    let index = find_round(&definition, &round_id)?;
    let round = &definition.rounds[index];
    let body = state.live.with(&competition.id, |results| {
        let results = results.get(&round.wcif_id).cloned().unwrap_or_default();
        let mut body = round.wcif.clone();
        body["round_id"] = json!(round.db_id);
        body["competitors"] = results
            .iter()
            .filter_map(|result| definition.registration(result.registration_id))
            .map(|registration| registration.to_live_json())
            .collect();
        body["results"] = results.iter().map(|result| result.to_json(round)).collect();
        body["state_hash"] = json!(model::Round::state_hash(&results));
        body["completed_competitors"] = json!(round.completed_competitors(&results));
        body
    });
    Ok(Json(body).into_response())
}

/// `GET /live/rounds`: every round's state. Public, but only for internal
/// scoretaking.
pub async fn rounds_info(
    State(state): State<SharedState>,
    Path(competition_id): Path<String>,
) -> ApiResult {
    let competition = v1::competition(&state, &competition_id)?;
    require_internal(&competition)?;
    let definition = definition(&state, &competition);
    let info: Vec<Value> = state.live.with(&competition.id, |results| {
        let rounds = rounds(&definition, results);
        (0..definition.rounds.len())
            .map(|index| rounds.info_json(index))
            .collect()
    });
    Ok(Json(json!({ "rounds": info })).into_response())
}

pub async fn open_round(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let index = find_round(&ctx.definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let definition = &ctx.definition;
    let round = &definition.rounds[index];
    let now = Utc::now();
    let body = state.live.with(&ctx.competition.id, |results| {
        let rounds = rounds(definition, results);
        match rounds.state(index) {
            RoundState::Pending => {
                return Err(bad_request(
                    "score taking is not finished in the previous round",
                ))
            }
            RoundState::Open | RoundState::Locked => return Err(bad_request("round already open")),
            RoundState::Blocked => {
                let clause = rounds
                    .insufficient_competitors_violation(index)
                    .unwrap_or_default();
                return Err(bad_request(&format!(
                    "regulation {}: {}",
                    clause,
                    model::insufficient_competitors_message(clause)
                )));
            }
            RoundState::Ready => {}
        }
        let proceeding: Vec<u64> = match round.source {
            Source::Registrations => definition
                .registrations
                .iter()
                .filter(|registration| {
                    registration.accepted
                        && registration.competing
                        && registration.event_ids.contains(&round.event_id)
                })
                .map(|registration| registration.id)
                .collect(),
            Source::Round(previous, _) => rounds.results[previous]
                .iter()
                .filter(|result| result.advancing)
                .map(|result| result.registration_id)
                .collect(),
        };
        let created: Vec<LiveResult> = proceeding
            .iter()
            .map(|id| LiveResult::empty(state.live.next_id(), *id, now))
            .collect();
        let created_rows = created.len();
        results.insert(round.wcif_id.clone(), created);
        let mut locked_rows = 0;
        if let Source::Round(previous, _) = round.source {
            if let Some(previous) = results.get_mut(&definition.rounds[previous].wcif_id) {
                for result in previous.iter_mut() {
                    result.locked = true;
                }
                locked_rows = previous.len();
            }
        }
        let state = rounds_state(definition, results, index);
        Ok(json!({
            "status": "ok",
            "locked_rows": locked_rows,
            "created_rows": created_rows,
            "state": state,
        }))
    })?;
    Ok(Json(body).into_response())
}

fn rounds_state(definition: &Definition, results: &RoundResults, index: usize) -> &'static str {
    rounds(definition, results).state(index).name()
}

/// Fails unless the round is open, as `clear` and `close` require.
fn require_open(
    definition: &Definition,
    results: &RoundResults,
    index: usize,
) -> Result<(), ApiError> {
    match rounds(definition, results).state(index) {
        RoundState::Locked => Err(bad_request("round is locked")),
        RoundState::Open => Ok(()),
        _ => Err(bad_request("round is not open")),
    }
}

pub async fn clear_round(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let index = find_round(&ctx.definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let definition = &ctx.definition;
    let body = state.live.with(&ctx.competition.id, |results| {
        require_open(definition, results, index)?;
        let round_results = results
            .get_mut(&definition.rounds[index].wcif_id)
            .expect("an open round has results");
        for result in round_results.iter_mut() {
            result.attempts.clear();
            result.best = 0;
            result.average = 0;
            result.advancing = false;
            result.advancing_questionable = false;
            result.local_pos = None;
            result.global_pos = None;
        }
        let recreated_rows = round_results.len();
        Ok(json!({
            "status": "ok",
            "recreated_rows": recreated_rows,
            "state": rounds_state(definition, results, index),
        }))
    })?;
    Ok(Json(body).into_response())
}

pub async fn close_round(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let index = find_round(&ctx.definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let definition = &ctx.definition;
    let round = &definition.rounds[index];
    let body = state.live.with(&ctx.competition.id, |results| {
        require_open(definition, results, index)?;
        let round_results = &results[&round.wcif_id];
        if round.completed_competitors(round_results) > 0 {
            return Err(bad_request("round has results entered"));
        }
        let deleted_count = results
            .remove(&round.wcif_id)
            .map_or(0, |results| results.len());
        if let Source::Round(previous, _) = round.source {
            if let Some(previous) = results.get_mut(&definition.rounds[previous].wcif_id) {
                for result in previous.iter_mut() {
                    result.locked = false;
                }
            }
        }
        Ok(json!({ "status": "ok", "deleted_count": deleted_count }))
    })?;
    Ok(Json(body).into_response())
}

/// Ids sent as `name[]=1&name[]=2` (or `name=1`) in the query, or as a JSON
/// array in the body, as Rails reads `params[name]`.
pub fn ids_param(query: Option<&str>, body: &Value, name: &str) -> Option<Vec<u64>> {
    if let Some(ids) = body[name].as_array() {
        return Some(
            ids.iter()
                .filter_map(|id| id.as_u64().or_else(|| id.as_str()?.parse().ok()))
                .collect(),
        );
    }
    let list_key = format!("{}[]", name);
    let ids: Vec<u64> = query
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _)| {
            let key = key.replace("%5B%5D", "[]");
            key == name || key == list_key
        })
        .filter_map(|(_, value)| value.parse().ok())
        .collect();
    (!ids.is_empty()).then_some(ids)
}

/// Shared by quitting one and several competitors.
fn quit(
    state: &SharedState,
    definition: &Definition,
    results: &mut RoundResults,
    index: usize,
    quitting: &[u64],
    advancing_ids: Option<Vec<u64>>,
    singular: bool,
) -> Result<usize, ApiError> {
    let round = &definition.rounds[index];
    let round_results = results.get(&round.wcif_id).cloned().unwrap_or_default();
    let quitting_results: Vec<&LiveResult> = round_results
        .iter()
        .filter(|result| quitting.contains(&result.registration_id))
        .collect();
    if singular && quitting_results.is_empty() {
        return Err(ApiError::not_found("LiveResult", quitting[0]));
    }
    let advancing_ids = advancing_ids.filter(|ids| !ids.is_empty());
    let first_round = round.source == Source::Registrations;
    let has_results = || {
        quitting_results
            .iter()
            .any(|result| !result.attempts.is_empty())
    };
    let check_first_round = || {
        if advancing_ids.is_some() && first_round {
            Err(bad_request("Can't advance next for first rounds"))
        } else {
            Ok(())
        }
    };
    // The WCA checks these in a different order for one and for several.
    if singular {
        check_first_round()?;
    }
    if has_results() {
        return Err(bad_request("Cannot quit competitor with results"));
    }
    if !singular {
        check_first_round()?;
    }

    let previous = match round.source {
        Source::Round(previous, _) => Some(previous),
        Source::Registrations => None,
    };
    let to_advance: Option<Vec<LiveResult>> = match (&advancing_ids, previous) {
        (Some(_), Some(previous)) => {
            let previous_results = results
                .get(&definition.rounds[previous].wcif_id)
                .cloned()
                .unwrap_or_default();
            Some(
                round
                    .next_participating_without(
                        &definition.rounds[previous],
                        &previous_results,
                        quitting,
                    )
                    .into_iter()
                    .cloned()
                    .collect(),
            )
        }
        _ => None,
    };
    if let (Some(advancing_ids), Some(to_advance)) = (&advancing_ids, &to_advance) {
        let should: Vec<u64> = to_advance
            .iter()
            .map(|result| result.registration_id)
            .collect();
        if *advancing_ids != should {
            let message = if singular {
                "The advancing competitor doesn't match who should be advancing."
            } else {
                "The advancing competitors don't match who should be advancing."
            };
            let previous_round = &definition.rounds[previous.expect("only later rounds advance")];
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                body: json!({
                    "status": message,
                    "should_advance": to_advance
                        .iter()
                        .map(|result| result.to_json(previous_round))
                        .collect::<Vec<_>>(),
                }),
            });
        }
    }

    let now = Utc::now();
    let mut updated: Vec<LiveResult> = round_results
        .into_iter()
        .filter(|result| !quitting.contains(&result.registration_id))
        .collect();
    for result in to_advance.iter().flatten() {
        updated.push(LiveResult::empty(
            state.live.next_id(),
            result.registration_id,
            now,
        ));
    }
    recompute(definition, index, &mut updated, false);
    results.insert(round.wcif_id.clone(), updated);

    let Some(previous) = previous else {
        return Ok(quitting.len());
    };
    if let Some(previous_results) = results.get_mut(&definition.rounds[previous].wcif_id) {
        for result in previous_results.iter_mut() {
            if to_advance
                .iter()
                .flatten()
                .any(|advancing| advancing.id == result.id)
            {
                result.advancing = true;
            }
            if quitting.contains(&result.registration_id) {
                result.quit = true;
                result.advancing = false;
                result.advancing_questionable = false;
            }
        }
    }
    Ok(if singular { 1 } else { quitting.len() })
}

/// `DELETE /live/rounds/:round_id/:registration_id`, optionally with the
/// `advancing_ids` the client expects to proceed in their place.
pub async fn quit_competitor(
    State(state): State<SharedState>,
    Path((competition_id, round_id, registration_id)): Path<(String, String, u64)>,
    RawQuery(query): RawQuery,
    token: Token,
    body: Option<Json<Value>>,
) -> ApiResult {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    let advancing_ids = ids_param(query.as_deref(), &body, "advancing_ids");
    let ctx = scoretaking(&state, &competition_id, &token)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let index = find_round(&ctx.definition, &round_id)?;
    let quit_count = state.live.with(&ctx.competition.id, |results| {
        quit(
            &state,
            &ctx.definition,
            results,
            index,
            &[registration_id],
            advancing_ids,
            true,
        )
    })?;
    Ok(Json(json!({ "status": "ok", "quit": quit_count })).into_response())
}

pub async fn bulk_quit_competitors(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    token: Token,
    body: Option<Json<Value>>,
) -> ApiResult {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let registration_ids =
        ids_param(query.as_deref(), &body, "registration_ids").ok_or_else(parameter_missing)?;
    let advancing_ids = ids_param(query.as_deref(), &body, "advancing_ids");
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let index = find_round(&ctx.definition, &round_id)?;
    let quit_count = state.live.with(&ctx.competition.id, |results| {
        quit(
            &state,
            &ctx.definition,
            results,
            index,
            &registration_ids,
            advancing_ids,
            false,
        )
    })?;
    Ok(Json(json!({ "status": "ok", "quit": quit_count })).into_response())
}

/// `GET /live/rounds/:round_id/next_if_quit?registration_id=`: who would
/// proceed in that competitor's place. Nobody, one, or several when tied.
pub async fn next_if_quit(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let registration_id = ids_param(query.as_deref(), &Value::Null, "registration_id")
        .and_then(|ids| ids.first().copied())
        .ok_or_else(parameter_missing)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let index = find_round(&ctx.definition, &round_id)?;
    let definition = &ctx.definition;
    let round = &definition.rounds[index];
    let next: Vec<u64> = state.live.with(&ctx.competition.id, |results| {
        let Source::Round(previous, _) = round.source else {
            return Vec::new();
        };
        let previous_round = &definition.rounds[previous];
        let empty = Vec::new();
        let previous_results = results.get(&previous_round.wcif_id).unwrap_or(&empty);
        round
            .next_participating_without(previous_round, previous_results, &[registration_id])
            .iter()
            .map(|result| result.registration_id)
            .collect()
    });
    let next_advancing = next
        .iter()
        .map(|id| {
            definition
                .registration(*id)
                .map(|registration| registration.to_live_json())
                .ok_or_else(|| ApiError::not_found("Registration", id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "status": "ok", "next_advancing": next_advancing })).into_response())
}

/// `GET /live/rounds/:round_id/addable_competitors`: for a first round,
/// anyone accepted to compete; later, everyone from the previous round.
pub async fn addable_competitors(
    State(state): State<SharedState>,
    Path((competition_id, round_id)): Path<(String, String)>,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let index = find_round(&ctx.definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let definition = &ctx.definition;
    let registrations: Vec<Value> = match definition.rounds[index].source {
        Source::Registrations => definition
            .registrations
            .iter()
            .filter(|registration| registration.accepted && registration.competing)
            .map(|registration| registration.to_v2_json())
            .collect(),
        Source::Round(previous, _) => state.live.with(&ctx.competition.id, |results| {
            results
                .get(&definition.rounds[previous].wcif_id)
                .into_iter()
                .flatten()
                .filter_map(|result| definition.registration(result.registration_id))
                .map(|registration| registration.to_v2_json())
                .collect()
        }),
    };
    Ok(Json(json!({ "registrations": registrations, "colinked_status": [] })).into_response())
}

/// `PUT /live/rounds/:round_id/:registration_id`: adds a competitor to the
/// round if it is open; otherwise does nothing, like the WCA.
pub async fn add_competitor(
    State(state): State<SharedState>,
    Path((competition_id, round_id, registration_id)): Path<(String, String, u64)>,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let definition = &ctx.definition;
    let registration = definition
        .registration(registration_id)
        .ok_or_else(|| ApiError::not_found("Registration", registration_id))?;
    let index = find_round(definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    state.live.with(&ctx.competition.id, |results| {
        if rounds(definition, results).state(index) != RoundState::Open {
            return;
        }
        let round_results = results
            .get_mut(&definition.rounds[index].wcif_id)
            .expect("an open round has results");
        if round_results
            .iter()
            .all(|result| result.registration_id != registration_id)
        {
            round_results.push(LiveResult::empty(
                state.live.next_id(),
                registration_id,
                Utc::now(),
            ));
            recompute(definition, index, round_results, false);
        }
    });
    Ok(Json(json!({ "status": "ok", "competitor": registration.to_live_json() })).into_response())
}

/// `PUT /live/rounds/:round_id/:registration_id/clear`: removes a
/// competitor's attempts. A locked result keeps whether it proceeded.
pub async fn clear_competitor(
    State(state): State<SharedState>,
    Path((competition_id, round_id, registration_id)): Path<(String, String, u64)>,
    token: Token,
) -> ApiResult {
    let ctx = scoretaking(&state, &competition_id, &token)?;
    let definition = &ctx.definition;
    let index = find_round(definition, &round_id)?;
    require_scoretake(&state, &ctx.competition, &ctx.caller)?;
    let deleted = state.live.with(&ctx.competition.id, |results| {
        let round_results = results
            .get_mut(&definition.rounds[index].wcif_id)
            .ok_or_else(|| ApiError::not_found("LiveResult", registration_id))?;
        let result = round_results
            .iter_mut()
            .find(|result| result.registration_id == registration_id)
            .ok_or_else(|| ApiError::not_found("LiveResult", registration_id))?;
        let deleted = result.attempts.len();
        let changed = result.best != 0 || result.average != 0;
        let locked = result.locked;
        result.attempts.clear();
        result.best = 0;
        result.average = 0;
        if !locked {
            result.advancing = false;
            result.advancing_questionable = false;
        }
        if changed {
            recompute(definition, index, round_results, locked);
        }
        Ok(deleted)
    })?;
    Ok(Json(json!({ "status": "ok", "deleted_attempts": deleted })).into_response())
}
