//! `GET /api/v1/persons/:wca_id`: a person's public profile, as the WCA's
//! `Api::V1::PersonsController#show` (`Person#to_v1_json`) gives it.
//!
//! It is built from what `/api/v0/persons/:wca_id` knows about the person.
//! dummy-wca has no results, so the parts the WCA works out from them are
//! empty: no solves, championship podiums or previous names.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Map, Value};

use super::{ApiError, ApiResult};
use crate::generator::events::EVENTS;
use crate::handler::person_entry;
use crate::state::SharedState;

/// What `find_by!` says when no person has the WCA ID.
const NOT_FOUND: &str =
    "Couldn't find Person with [WHERE `persons`.`sub_id` = ? AND `persons`.`wca_id` = ?]";

/// The championship levels the WCA serializes, always all of them.
const CHAMPIONSHIP_LEVELS: [&str; 4] = ["world", "continental", "greater_china", "national"];

/// Public: a person's results are public data.
pub async fn show(State(state): State<SharedState>, Path(wca_id): Path<String>) -> ApiResult {
    let entry = person_entry(&state, &wca_id).ok_or_else(|| {
        ApiError::with_data(
            StatusCode::NOT_FOUND,
            NOT_FOUND,
            json!({ "model": "Person", "id": null }),
        )
    })?;
    Ok(Json(v1_person(&entry)).into_response())
}

/// `Person#to_v1_json` from a `/api/v0/persons/:wca_id` entry.
fn v1_person(entry: &Value) -> Value {
    let person = &entry["person"];
    let personal_records = entry["personal_records"].as_object();
    let podiums: Map<String, Value> = CHAMPIONSHIP_LEVELS
        .iter()
        .map(|level| (level.to_string(), json!([])))
        .collect();
    json!({
        "wca_id": person["wca_id"],
        "name": person["name"],
        "gender": person["gender"],
        "url": person["url"],
        "country_iso2": person["country_iso2"],
        "delegate_status": person["delegate_status"],
        "teams": teams(&person["teams"]),
        "competition_count": entry["competition_count"],
        "event_ids_with_results": event_ids(personal_records),
        "medals": entry["medals"],
        "records": entry["records"],
        "completed_solves_count": 0,
        "avatar": {
            "url": person["avatar"]["url"],
            "thumb_url": person["avatar"]["thumb_url"],
            "is_default": person["avatar"]["is_default"],
        },
        "previous_persons": [],
        "ranks_by_event": ranks_by_event(personal_records),
        "championship_podium_results": podiums,
    })
}

/// `UserRole#v1_team_membership`: the team and the member's status in it.
/// The board has no ranks among its members, so its status is null.
fn teams(teams: &Value) -> Vec<Value> {
    let flag = |team: &Value, key: &str| team[key].as_bool().unwrap_or_default();
    teams
        .as_array()
        .into_iter()
        .flatten()
        .map(|team| {
            let status = if team["friendly_id"] == "board" {
                None
            } else if flag(team, "leader") {
                Some("leader")
            } else if flag(team, "senior_member") {
                Some("senior_member")
            } else {
                Some("member")
            };
            json!({ "friendly_id": team["friendly_id"], "status": status })
        })
        .collect()
}

/// Events in the WCA's order; any it no longer holds come after, by id.
fn event_order(event_id: &str) -> (usize, String) {
    let position = EVENTS.iter().position(|event| event.id == event_id);
    (position.unwrap_or(EVENTS.len()), event_id.to_string())
}

/// Every event the person has a personal record in. The WCA also counts
/// events without a success, which dummy-wca can't know about.
fn event_ids(personal_records: Option<&Map<String, Value>>) -> Vec<String> {
    let mut ids: Vec<String> = personal_records
        .into_iter()
        .flat_map(|records| records.keys().cloned())
        .collect();
    ids.sort_by_key(|id| event_order(id));
    ids
}

/// `Person#ranks_by_event`: official events with a single, each with its
/// average if there is one, as `{best, world_rank, continent_rank, country_rank}`.
fn ranks_by_event(personal_records: Option<&Map<String, Value>>) -> Map<String, Value> {
    let rank = |rank: &Value| {
        json!({
            "best": rank["best"],
            "world_rank": rank["world_rank"],
            "continent_rank": rank["continent_rank"],
            "country_rank": rank["country_rank"],
        })
    };
    personal_records
        .into_iter()
        .flatten()
        .filter(|(event_id, _)| EVENTS.iter().any(|event| event.id == event_id.as_str()))
        .filter(|(_, records)| records["single"].is_object())
        .map(|(event_id, records)| {
            let mut ranks = Map::new();
            ranks.insert("single".into(), rank(&records["single"]));
            if records["average"].is_object() {
                ranks.insert("average".into(), rank(&records["average"]));
            }
            (event_id.clone(), Value::Object(ranks))
        })
        .collect()
}
