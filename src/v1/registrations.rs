//! `PATCH /api/v1/registrations/:id`, for changing a registration's events.
//! dummy-wca only changes `competing.event_ids`; the rest of the WCA's
//! registration update is out of scope.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};

use super::{can_manage, require_user, ApiError, ApiResult, Token};
use crate::generator::{
    model::Person,
    wcif::{self, Version},
    Competition,
};
use crate::state::SharedState;

// `Registrations::ErrorCodes` used here.
const USER_INSUFFICIENT_PERMISSIONS: i64 = -2003;
const REGISTRATION_NOT_FOUND: i64 = -3000;
pub const INVALID_REQUEST_DATA: i64 = -4000;
const USER_EDITS_NOT_ALLOWED: i64 = -4001;
const INVALID_EVENT_SELECTION: i64 = -4004;

/// Third-party clients need this scope to change registrations.
const MANAGE_REGISTRATIONS_SCOPE: &str = "manage_registrations";

/// The fields of `competing` the WCA accepts; dummy-wca acts on `event_ids`.
const COMPETING_FIELDS: [&str; 5] = [
    "status",
    "comment",
    "event_ids",
    "admin_comment",
    "waiting_list_position",
];

/// The registration with this id, in whichever generated competition has it.
fn find(state: &SharedState, registration_id: u64) -> Option<(Arc<Competition>, usize)> {
    state.competitions().into_iter().find_map(|competition| {
        let index = competition
            .persons
            .iter()
            .position(|person| person.registration.wca_registration_id == registration_id)?;
        Some((competition, index))
    })
}

/// The events held at the competition, after any WCIF patches.
fn held_events(state: &SharedState, competition: &Competition) -> Vec<String> {
    let mut document = wcif::wcif(competition, Version::V2);
    state.apply_patches(&competition.id, &mut document);
    document["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|event| event["id"].as_str().map(str::to_string))
        .collect()
}

fn event_ids(state: &SharedState, competition: &Competition, person: &Person) -> Vec<String> {
    state
        .registration_events(&competition.id, person.registration.wca_registration_id)
        .unwrap_or_else(|| {
            person
                .registration
                .event_ids
                .iter()
                .map(|id| id.to_string())
                .collect()
        })
}

/// `Registration#to_v2_json(admin: true)`, without payments.
fn registration_json(state: &SharedState, competition: &Competition, person: &Person) -> Value {
    let status = match (person.registration.is_competing, person.registration.status) {
        (false, _) => "non_competing",
        (true, "deleted") => "cancelled",
        (true, status) => status,
    };
    json!({
        "id": person.registration.wca_registration_id,
        "user": {
            "id": person.wca_user_id,
            "wca_id": person.wca_id,
            "name": person.name,
            "gender": person.gender,
            "country_iso2": person.country_iso2,
        },
        "user_id": person.wca_user_id,
        "registrant_id": person.registrant_id,
        "competing": {
            "event_ids": event_ids(state, competition, person),
            "registration_status": status,
            "comment": "",
            "admin_comment": "",
        },
        "guests": 0,
    })
}

fn invalid_events(data: Value) -> ApiError {
    ApiError::with_data(
        StatusCode::UNPROCESSABLE_ENTITY,
        INVALID_EVENT_SELECTION,
        data,
    )
}

/// Checks the new events as the WCA's registration validations do. Managers
/// skip the event change deadline but not the events-per-registration limit.
fn check_events(
    event_ids: &[String],
    held: &[String],
    is_competing: bool,
    limit: Option<u32>,
) -> Result<(), ApiError> {
    let not_held: Vec<&String> = event_ids
        .iter()
        .filter(|event_id| !held.contains(event_id))
        .collect();
    if !not_held.is_empty() {
        return Err(invalid_events(json!(not_held)));
    }
    if is_competing && event_ids.is_empty() {
        return Err(invalid_events(json!({
            "error": "blank",
            "frontend_code": INVALID_EVENT_SELECTION,
        })));
    }
    if let Some(limit) = limit.filter(|limit| event_ids.len() > *limit as usize) {
        return Err(invalid_events(json!({
            "error": "too_long",
            "count": limit,
            "frontend_code": INVALID_EVENT_SELECTION,
        })));
    }
    Ok(())
}

pub async fn update(
    State(state): State<SharedState>,
    Path(registration_id): Path<String>,
    token: Token,
    body: Option<Json<Value>>,
) -> ApiResult {
    let caller = require_user(&token)?;
    if !caller.has_scope(MANAGE_REGISTRATIONS_SCOPE) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            USER_INSUFFICIENT_PERMISSIONS,
        ));
    }
    let not_found = || ApiError::new(StatusCode::NOT_FOUND, REGISTRATION_NOT_FOUND);
    let registration_id: u64 = registration_id.parse().map_err(|_| not_found())?;
    let (competition, index) = find(&state, registration_id).ok_or_else(not_found)?;
    let person = &competition.persons[index];
    let body = body.map(|Json(body)| body).unwrap_or_default();

    let is_manager = can_manage(&state, &competition, caller.wca_id);
    let is_self = person
        .wca_id
        .as_deref()
        .is_some_and(|wca_id| wca_id.eq_ignore_ascii_case(caller.wca_id));
    if !is_manager && !is_self {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            USER_INSUFFICIENT_PERMISSIONS,
        ));
    }
    // Generated competitions have no event change deadline, so competitors
    // may edit until the competition starts.
    let today = Utc::now().with_timezone(&competition.timezone).date_naive();
    if !is_manager && today >= competition.start_date {
        return Err(ApiError::new(StatusCode::FORBIDDEN, USER_EDITS_NOT_ALLOWED));
    }

    let Some(competing) = body.get("competing") else {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            body: json!({
                "status": "bad request",
                "message": "You need to supply at least one lane",
            }),
        });
    };
    let competing = competing
        .as_object()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, INVALID_REQUEST_DATA))?;
    if !is_manager
        && ["admin_comment", "waiting_list_position"]
            .iter()
            .any(|field| competing.contains_key(*field))
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            USER_INSUFFICIENT_PERMISSIONS,
        ));
    }
    let unsupported = body
        .as_object()
        .into_iter()
        .flat_map(|body| body.keys())
        .filter(|key| *key != "competing")
        .chain(competing.keys().filter(|key| *key != "event_ids"))
        .find(|key| *key == "guests" || COMPETING_FIELDS.contains(&key.as_str()));
    if let Some(field) = unsupported {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            body: json!({
                "error": INVALID_REQUEST_DATA,
                "message": format!("dummy-wca can only change competing.event_ids, not {}", field),
            }),
        });
    }

    if let Some(event_ids) = competing.get("event_ids") {
        let event_ids: Vec<String> = event_ids
            .as_array()
            .and_then(|ids| {
                ids.iter()
                    .map(|id| id.as_str().map(str::to_string))
                    .collect()
            })
            .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, INVALID_REQUEST_DATA))?;
        let mut unique: Vec<String> = Vec::new();
        for event_id in event_ids {
            if !unique.contains(&event_id) {
                unique.push(event_id);
            }
        }
        check_events(
            &unique,
            &held_events(&state, &competition),
            person.registration.is_competing,
            competition.events_per_registration_limit,
        )?;
        state.set_registration_events(&competition.id, registration_id, unique);
    }

    Ok(Json(json!({
        "status": "ok",
        "registration": registration_json(&state, &competition, person),
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn events_must_be_held_and_within_the_limit() {
        let held = ids(&["333", "222", "pyram"]);
        assert!(check_events(&ids(&["333"]), &held, true, None).is_ok());
        let error = check_events(&ids(&["333", "777"]), &held, true, None).unwrap_err();
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error.body["error"], INVALID_EVENT_SELECTION);
        assert_eq!(error.body["data"], json!(["777"]));
        let error = check_events(&ids(&[]), &held, true, None).unwrap_err();
        assert_eq!(error.body["data"]["error"], "blank");
        assert!(check_events(&ids(&[]), &held, false, None).is_ok());
        let error = check_events(&held, &held, true, Some(2)).unwrap_err();
        assert_eq!(error.body["data"]["count"], 2);
        assert!(check_events(&held, &held, true, Some(3)).is_ok());
    }
}
