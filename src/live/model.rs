//! The WCA's live results rules, ported from its `Round`, `LiveResult`,
//! `Resultable`, `SolveTime` and `ResultConditions` models: how a round's
//! results are computed, ranked and advanced, and what state a round is in.
//!
//! Rounds come from the competition's WCIF (v2), so a WCIF patch changes
//! them too. Linked rounds and head-to-head rounds aren't supported.

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};

/// A round format, from the WCA's `formats.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub id: char,
    pub sort_by_average: bool,
    pub expected_solve_count: usize,
    trim_fastest_n: usize,
    trim_slowest_n: usize,
}

impl Format {
    pub fn find(id: &str) -> Option<Format> {
        let (sort_by_average, expected_solve_count, trim) = match id {
            "1" => (false, 1, 0),
            "2" => (false, 2, 0),
            "3" => (false, 3, 0),
            "5" => (false, 5, 1),
            "a" => (true, 5, 1),
            "m" => (true, 3, 0),
            _ => return None,
        };
        Some(Format {
            id: id.chars().next()?,
            sort_by_average,
            expected_solve_count,
            trim_fastest_n: trim,
            trim_slowest_n: trim,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Single,
    Average,
}

/// How many competitors proceed, from a WCIF `resultCondition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Ranking(i64),
    Percent(i64),
    /// `None` is "any result".
    ResultAchieved(Scope, Option<i64>),
}

impl Condition {
    pub fn from_wcif(condition: &Value) -> Option<Condition> {
        let value = condition["value"].as_i64();
        match condition["type"].as_str()? {
            "ranking" => Some(Condition::Ranking(value?)),
            "percent" => Some(Condition::Percent(value?)),
            "resultAchieved" => {
                let scope = match condition["scope"].as_str() {
                    Some("average") => Scope::Average,
                    _ => Scope::Single,
                };
                Some(Condition::ResultAchieved(scope, value))
            }
            _ => None,
        }
    }

    fn max_advancing(&self, results: &[&LiveResult]) -> usize {
        match *self {
            Condition::Ranking(value) => value.max(0) as usize,
            Condition::Percent(percent) => {
                let valid = results.iter().filter(|result| result.best > 0).count();
                let proceeds = results.len() * percent.max(0) as usize / 100;
                valid.min(proceeds)
            }
            Condition::ResultAchieved(scope, value) => results
                .iter()
                .filter(|result| {
                    let field = match scope {
                        Scope::Single => result.best,
                        Scope::Average => result.average,
                    };
                    field > 0 && value.is_none_or(|value| field < value)
                })
                .count(),
        }
    }

    /// The Regulations let at most 75% of competitors proceed.
    fn max_qualifying(&self, results: &[&LiveResult]) -> usize {
        self.max_advancing(results)
            .min(regulations_boundary(results.len()))
    }

    /// Who proceeds, from results sorted by their potential. Ties at the
    /// boundary all proceed, unless that breaks the 75% rule, in which case
    /// none of them do. Incomplete results never proceed.
    fn apply<'a>(&self, round: &Round, results: &[&'a LiveResult]) -> Vec<&'a LiveResult> {
        let qualifying_rank = self.max_qualifying(results).min(results.len());
        let top = &results[..qualifying_rank];
        let Some(last) = top.last() else {
            return Vec::new();
        };
        let cutoff = round.potential(last);
        let tied: Vec<&LiveResult> = results[qualifying_rank..]
            .iter()
            .take_while(|result| round.potential(result) == cutoff)
            .copied()
            .collect();
        let with_ties: Vec<&LiveResult> = top.iter().copied().chain(tied).collect();
        let advancing = if with_ties.len() > regulations_boundary(results.len()) {
            top.iter()
                .copied()
                .filter(|result| round.potential(result) != cutoff)
                .collect()
        } else {
            with_ties
        };
        advancing
            .into_iter()
            .filter(|result| round.is_complete(result))
            .collect()
    }
}

fn regulations_boundary(count: usize) -> usize {
    count * 3 / 4
}

/// Where a round's competitors come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Registrations,
    /// The previous round, and who of it proceeds.
    Round(usize, Option<Condition>),
}

/// A round as the WCIF defines it.
#[derive(Clone, Debug)]
pub struct Round {
    /// `333-r1`.
    pub wcif_id: String,
    /// The numeric id the WCA's database would give it.
    pub db_id: u64,
    pub event_id: String,
    pub number: u32,
    pub total_rounds: u32,
    pub format: Format,
    /// Number of attempts, and the result to beat.
    pub cutoff: Option<(usize, i64)>,
    pub source: Source,
    /// The WCIF round without its results, as the live endpoints show it.
    pub wcif: Value,
}

/// A registration, as the live endpoints need it.
#[derive(Clone, Debug)]
pub struct Registration {
    pub id: u64,
    pub user_id: u64,
    pub registrant_id: Option<u64>,
    pub name: String,
    pub wca_id: Option<String>,
    pub country_iso2: String,
    pub gender: String,
    pub event_ids: Vec<String>,
    pub accepted: bool,
    pub competing: bool,
}

impl Registration {
    /// `Registration#to_live_json`.
    pub fn to_live_json(&self) -> Value {
        json!({
            "id": self.id,
            "user_id": self.user_id,
            "registrant_id": self.registrant_id,
            "name": self.name,
            "country_iso2": self.country_iso2,
            "wca_id": self.wca_id,
        })
    }

    /// `Registration#to_v2_json`.
    pub fn to_v2_json(&self) -> Value {
        json!({
            "id": self.id,
            "user": {
                "id": self.user_id,
                "wca_id": self.wca_id,
                "name": self.name,
                "gender": self.gender,
                "country_iso2": self.country_iso2,
            },
            "user_id": self.user_id,
            "registrant_id": self.registrant_id,
            "competing": { "event_ids": self.event_ids },
        })
    }
}

/// What the WCIF says about a competition's rounds and registrations.
#[derive(Clone, Debug, Default)]
pub struct Definition {
    pub rounds: Vec<Round>,
    pub registrations: Vec<Registration>,
}

impl Definition {
    pub fn from_wcif(wcif: &Value) -> Definition {
        let mut rounds = Vec::new();
        for event in wcif["events"].as_array().into_iter().flatten() {
            let event_id = event["id"].as_str().unwrap_or_default();
            let event_rounds: Vec<&Value> =
                event["rounds"].as_array().into_iter().flatten().collect();
            let first_index = rounds.len();
            for (index, wcif_round) in event_rounds.iter().enumerate() {
                let Some(format) = wcif_round["format"].as_str().and_then(Format::find) else {
                    continue;
                };
                let cutoff = &wcif_round["cutoff"];
                let cutoff = cutoff["numberOfAttempts"].as_u64().map(|attempts| {
                    let result = cutoff["resultValue"]
                        .as_i64()
                        .or(cutoff["attemptResult"].as_i64())
                        .unwrap_or_default();
                    (attempts as usize, result)
                });
                let source = &wcif_round["participationRuleset"]["participationSource"];
                let source = match (index, source["type"].as_str()) {
                    (0, _) | (_, Some("registrations")) => Source::Registrations,
                    _ => Source::Round(
                        first_index + index - 1,
                        Condition::from_wcif(&source["resultCondition"]),
                    ),
                };
                let mut shown = wcif_round.as_object().cloned().unwrap_or_default();
                shown.remove("results");
                rounds.push(Round {
                    wcif_id: wcif_round["id"].as_str().unwrap_or_default().to_string(),
                    db_id: rounds.len() as u64 + 1,
                    event_id: event_id.to_string(),
                    number: index as u32 + 1,
                    total_rounds: event_rounds.len() as u32,
                    format,
                    cutoff,
                    source,
                    wcif: compact_blank(Value::Object(shown)),
                });
            }
        }
        let registrations = wcif["persons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|person| {
                let registration = &person["registration"];
                Some(Registration {
                    id: registration["wcaRegistrationId"].as_u64()?,
                    user_id: person["wcaUserId"].as_u64().unwrap_or_default(),
                    registrant_id: person["registrantId"].as_u64(),
                    name: person["name"].as_str().unwrap_or_default().to_string(),
                    wca_id: person["wcaId"].as_str().map(str::to_string),
                    country_iso2: person["countryIso2"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    gender: person["gender"].as_str().unwrap_or_default().to_string(),
                    event_ids: registration["eventIds"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|id| id.as_str().map(str::to_string))
                        .collect(),
                    accepted: registration["status"] == "accepted",
                    competing: registration["isCompeting"].as_bool().unwrap_or(true),
                })
            })
            .collect();
        Definition {
            rounds,
            registrations,
        }
    }

    pub fn round(&self, wcif_id: &str) -> Option<usize> {
        self.rounds
            .iter()
            .position(|round| round.wcif_id == wcif_id)
    }

    pub fn registration(&self, id: u64) -> Option<&Registration> {
        self.registrations
            .iter()
            .find(|registration| registration.id == id)
    }

    /// The round after this one in its event, if any.
    pub fn next_round(&self, index: usize) -> Option<usize> {
        self.rounds.iter().position(
            |round| matches!(round.source, Source::Round(previous, _) if previous == index),
        )
    }
}

/// Like Rails' `compact_blank`: drops nulls, `false`, and empty strings,
/// arrays and objects.
fn compact_blank(value: Value) -> Value {
    let Value::Object(map) = value else {
        return value;
    };
    let kept: Map<String, Value> = map
        .into_iter()
        .filter(|(_, value)| match value {
            Value::Null | Value::Bool(false) => false,
            Value::String(text) => !text.is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        })
        .collect();
    Value::Object(kept)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub attempt_number: i64,
    pub value: i64,
}

#[derive(Clone, Debug)]
pub struct LiveResult {
    pub id: u64,
    pub registration_id: u64,
    pub attempts: Vec<Attempt>,
    pub best: i64,
    pub average: i64,
    pub advancing: bool,
    pub advancing_questionable: bool,
    pub locked: bool,
    pub quit: bool,
    pub local_pos: Option<u32>,
    pub global_pos: Option<u32>,
    pub last_attempt_entered_at: DateTime<Utc>,
}

impl LiveResult {
    pub fn empty(id: u64, registration_id: u64, now: DateTime<Utc>) -> LiveResult {
        LiveResult {
            id,
            registration_id,
            attempts: Vec::new(),
            best: 0,
            average: 0,
            advancing: false,
            advancing_questionable: false,
            locked: false,
            quit: false,
            local_pos: None,
            global_pos: None,
            last_attempt_entered_at: now,
        }
    }

    /// The default `LiveResult` JSON.
    pub fn to_json(&self, round: &Round) -> Value {
        json!({
            "result_id": self.id,
            "registration_id": self.registration_id,
            "round_wcif_id": round.wcif_id,
            "event_id": round.event_id,
            "attempts": self.attempts_json(),
            "best": self.best,
            "average": self.average,
            "single_record_tag": null,
            "average_record_tag": null,
            "advancing": self.advancing,
            "advancing_questionable": self.advancing_questionable,
            "global_pos": self.global_pos,
            "local_pos": self.local_pos,
            "last_attempt_entered_at": timestamp(self.last_attempt_entered_at),
        })
    }

    /// `LiveResult#to_live_state`, which the state hash is taken from.
    pub fn to_live_state(&self) -> Value {
        json!({
            "advancing": self.advancing,
            "advancing_questionable": self.advancing_questionable,
            "average": self.average,
            "average_record_tag": null,
            "best": self.best,
            "registration_id": self.registration_id,
            "last_attempt_entered_at": timestamp(self.last_attempt_entered_at),
            "single_record_tag": null,
            "live_attempts": self.attempts_json(),
        })
    }

    fn attempts_json(&self) -> Vec<Value> {
        self.attempts
            .iter()
            .map(|attempt| json!({ "value": attempt.value, "attempt_number": attempt.attempt_number }))
            .collect()
    }
}

/// How Rails renders a timestamp in JSON.
pub fn timestamp(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// `SolveTime#to_orderable`: complete results first, then DNF, DNS and skipped.
fn orderable(value: i64) -> (bool, bool, bool, i64) {
    (value == 0, value == -2, value == -1, value)
}

impl Round {
    fn values_for_sorting(&self, result: &LiveResult) -> Vec<(bool, bool, bool, i64)> {
        if self.format.sort_by_average {
            vec![orderable(result.average), orderable(result.best)]
        } else {
            vec![orderable(result.best)]
        }
    }

    /// Complete results by their values; incomplete ones as if they would
    /// still get the best possible result.
    fn potential(&self, result: &LiveResult) -> Vec<(bool, bool, bool, i64)> {
        if self.is_complete(result) {
            self.values_for_sorting(result)
        } else {
            let best_possible = orderable(1);
            vec![best_possible; if self.format.sort_by_average { 2 } else { 1 }]
        }
    }

    fn didnt_meet_cutoff(&self, result: &LiveResult) -> bool {
        let Some((attempts, to_beat)) = self.cutoff else {
            return false;
        };
        !result.attempts.is_empty()
            && !result
                .attempts
                .iter()
                .take(attempts)
                .any(|attempt| attempt.value > 0 && attempt.value < to_beat)
    }

    pub fn is_complete(&self, result: &LiveResult) -> bool {
        result.attempts.len() == self.format.expected_solve_count || self.didnt_meet_cutoff(result)
    }

    fn rank_key(&self, result: &LiveResult) -> (bool, bool, i64, i64) {
        if self.format.sort_by_average {
            let primary = if result.average > 0 {
                result.average
            } else {
                result.best
            };
            (result.average <= 0, result.best <= 0, primary, result.best)
        } else {
            (result.best <= 0, false, result.best, 0)
        }
    }

    /// `recompute_local_pos`: `RANK()` over results with attempts; others
    /// get no position. Without linked rounds the global position is the same.
    fn recompute_positions(&self, results: &mut [LiveResult]) {
        let mut keys: Vec<(bool, bool, i64, i64)> = results
            .iter()
            .filter(|result| result.best != 0)
            .map(|result| self.rank_key(result))
            .collect();
        keys.sort();
        for result in results.iter_mut() {
            let pos = (result.best != 0).then(|| {
                let key = self.rank_key(result);
                keys.iter().filter(|other| **other < key).count() as u32 + 1
            });
            result.local_pos = pos;
            result.global_pos = pos;
        }
    }

    fn recompute_advancing(&self, condition: Option<Condition>, results: &mut [LiveResult]) {
        let condition = if self.number == self.total_rounds {
            // A final's "advancing" marks the podium.
            Some(Condition::Ranking(3))
        } else {
            condition
        };
        let Some(condition) = condition else {
            return;
        };
        let mut sorted: Vec<&LiveResult> = results.iter().filter(|result| !result.locked).collect();
        sorted.sort_by_key(|result| self.potential(result));
        let advancing: Vec<u64> = condition
            .apply(self, &sorted)
            .iter()
            .map(|result| result.registration_id)
            .collect();
        let max_advancing = condition.max_qualifying(&sorted) as u32;
        for result in results.iter_mut() {
            let Some(global_pos) = result.global_pos.filter(|_| !result.locked) else {
                continue;
            };
            result.advancing = advancing.contains(&result.registration_id);
            result.advancing_questionable = global_pos <= max_advancing;
        }
    }

    /// `recompute_live_columns`: positions, then who proceeds unless told
    /// not to (when a locked result changed).
    pub fn recompute(
        &self,
        next_condition: Option<Condition>,
        results: &mut [LiveResult],
        skip_advancing: bool,
    ) {
        self.recompute_positions(results);
        if !skip_advancing {
            self.recompute_advancing(next_condition, results);
        }
    }

    pub fn completed_competitors(&self, results: &[LiveResult]) -> usize {
        results
            .iter()
            .filter(|result| self.is_complete(result))
            .count()
    }

    pub fn score_taking_done(&self, results: &[LiveResult]) -> bool {
        !results.is_empty() && self.completed_competitors(results) == results.len()
    }

    /// `Round#to_live_state`'s hash, which changes whenever a result does.
    pub fn state_hash(results: &[LiveResult]) -> String {
        let state: Vec<Value> = results.iter().map(LiveResult::to_live_state).collect();
        sha1_smol::Sha1::from(Value::from(state).to_string())
            .digest()
            .to_string()
    }

    /// `Round.next_participating_without`, ported from WCA Live: who of the
    /// previous round would proceed in place of the competitors who quit.
    /// Ties give several; nobody left gives none.
    pub fn next_participating_without<'a>(
        &self,
        previous: &Round,
        previous_results: &'a [LiveResult],
        quitting: &[u64],
    ) -> Vec<&'a LiveResult> {
        let Source::Round(_, condition) = self.source else {
            return Vec::new();
        };
        let candidate_ids: Vec<u64> = previous_results
            .iter()
            .filter(|result| !result.advancing && !result.quit)
            .map(|result| result.id)
            .collect();
        if candidate_ids.is_empty() {
            return Vec::new();
        }
        let mut ignored: Vec<u64> = previous_results
            .iter()
            .find(|result| result.advancing)
            .map(|result| result.id)
            .into_iter()
            .collect();
        for result in previous_results {
            if (quitting.contains(&result.registration_id) || result.quit)
                && !ignored.contains(&result.id)
            {
                ignored.push(result.id);
            }
        }
        // Everyone ignored is assumed to have got a DNF.
        let worst: Vec<LiveResult> = ignored
            .iter()
            .map(|id| LiveResult {
                best: -1,
                average: -1,
                ..LiveResult::empty(*id, 0, DateTime::<Utc>::MIN_UTC)
            })
            .collect();
        let mut hypothetical: Vec<&LiveResult> = previous_results
            .iter()
            .filter(|result| !ignored.contains(&result.id))
            .chain(worst.iter())
            .collect();
        hypothetical.sort_by_key(|result| previous.values_for_sorting(result));
        let Some(condition) = condition else {
            return Vec::new();
        };
        let proceeding: Vec<u64> = condition
            .apply(previous, &hypothetical)
            .iter()
            .map(|result| result.id)
            .filter(|id| candidate_ids.contains(id))
            .collect();
        previous_results
            .iter()
            .filter(|result| proceeding.contains(&result.id))
            .collect()
    }
}

/// `Round::STATE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Pending,
    Blocked,
    Ready,
    Open,
    Locked,
}

impl State {
    pub fn name(&self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Blocked => "blocked",
            State::Ready => "ready",
            State::Open => "open",
            State::Locked => "locked",
        }
    }
}

/// Regulation 9m: competitors a round needs for each number of rounds after it.
const MIN_COMPETITORS_PER_CLAUSE: [(&str, usize); 3] = [("9m1", 100), ("9m2", 16), ("9m3", 8)];

pub fn insufficient_competitors_message(clause: &str) -> &'static str {
    match clause {
        "9m1" => "a round with 99 or fewer competitors must have at most two subsequent rounds",
        "9m2" => "a round with 15 or fewer competitors must have at most one subsequent round",
        _ => "a round with 7 or fewer competitors must not have subsequent rounds",
    }
}

/// A competition's rounds with their live results, by round index.
pub struct Rounds<'a> {
    pub definition: &'a Definition,
    pub results: Vec<&'a [LiveResult]>,
}

impl Rounds<'_> {
    pub fn state(&self, index: usize) -> State {
        let round = &self.definition.rounds[index];
        let results = self.results[index];
        if !results.is_empty() && results.iter().all(|result| result.locked) {
            return State::Locked;
        }
        if !results.is_empty() {
            return State::Open;
        }
        if let Source::Round(previous, _) = round.source {
            if !self.definition.rounds[previous].score_taking_done(self.results[previous]) {
                return State::Pending;
            }
        }
        if self.insufficient_competitors_violation(index).is_some() {
            return State::Blocked;
        }
        State::Ready
    }

    /// The 9m clause opening this round would break, if any.
    pub fn insufficient_competitors_violation(&self, index: usize) -> Option<&'static str> {
        let round = &self.definition.rounds[index];
        let mut previous = match round.source {
            Source::Round(previous, _) => Some(previous),
            Source::Registrations => None,
        };
        let mut binding: Option<(u32, usize)> = None;
        while let Some(index) = previous {
            let previous_round = &self.definition.rounds[index];
            let competitors = self.results[index].len();
            let allowed = MIN_COMPETITORS_PER_CLAUSE
                .iter()
                .filter(|(_, min)| competitors >= *min)
                .count();
            let max_round = previous_round.number + allowed as u32;
            if binding.is_none_or(|(binding_max, _)| max_round < binding_max) {
                binding = Some((max_round, allowed));
            }
            previous = match previous_round.source {
                Source::Round(earlier, _) => Some(earlier),
                Source::Registrations => None,
            };
        }
        let (max_round, allowed) = binding?;
        if max_round.min(4) >= round.number {
            return None;
        }
        MIN_COMPETITORS_PER_CLAUSE
            .iter()
            .rev()
            .nth(allowed)
            .map(|(clause, _)| *clause)
    }

    pub fn competitor_count_needed(&self, index: usize) -> Option<usize> {
        let clause = self.insufficient_competitors_violation(index)?;
        MIN_COMPETITORS_PER_CLAUSE
            .iter()
            .find(|(name, _)| *name == clause)
            .map(|(_, min)| *min)
    }

    /// `Round#to_live_info_json`.
    pub fn info_json(&self, index: usize) -> Value {
        let round = &self.definition.rounds[index];
        let results = self.results[index];
        let state = self.state(index);
        let mut info = round.wcif.clone();
        info["state"] = json!(state.name());
        info["min_competitors_to_open"] = match round.number {
            1 => Value::Null,
            number => json!(MIN_COMPETITORS_PER_CLAUSE
                .iter()
                .rev()
                .nth(number as usize - 2)
                .map(|(_, min)| *min)),
        };
        if matches!(state, State::Open | State::Locked) {
            info["total_competitors"] = json!(results.len());
        }
        if state == State::Open {
            info["completed_competitors"] = json!(round.completed_competitors(results));
        }
        if state == State::Blocked {
            info["competitor_count_needed"] = json!(self.competitor_count_needed(index));
        }
        info
    }
}

#[cfg(test)]
mod tests;
