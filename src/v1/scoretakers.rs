//! `/api/v1/competitions/:id/scoretakers`: who besides the managers may
//! enter results, as on the WCA.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use axum::{http::StatusCode, response::IntoResponse};

use super::{
    competition, registrations::INVALID_REQUEST_DATA, require_manage, require_user, user_name,
    ApiError, ApiResult, Token,
};
use crate::state::SharedState;

/// `SCORETAKER_JSON`: `{user_id, name}`.
fn scoretaker_json(state: &SharedState, user_id: u64) -> Value {
    let name = user_name(state, user_id);
    json!({ "user_id": user_id, "name": name })
}

/// Any signed-in user may list them.
pub async fn index(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    token: Token,
) -> ApiResult {
    require_user(&token)?;
    let competition = competition(&state, &id)?;
    let list: Vec<Value> = state
        .scoretakers(&competition.id)
        .into_iter()
        .map(|user_id| scoretaker_json(&state, user_id))
        .collect();
    Ok(Json(list).into_response())
}

#[derive(Deserialize)]
pub struct NewScoretaker {
    user_id: Option<u64>,
}

pub async fn create(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    token: Token,
    body: Option<Json<NewScoretaker>>,
) -> ApiResult {
    let caller = require_user(&token)?;
    let competition = competition(&state, &id)?;
    require_manage(&state, &competition, caller.wca_id)?;
    let Some(user_id) = body.and_then(|Json(body)| body.user_id) else {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, INVALID_REQUEST_DATA));
    };
    if user_name(&state, user_id).is_none() {
        return Err(ApiError::not_found("User", user_id));
    }
    state.add_scoretaker(&competition.id, user_id);
    Ok(Json(scoretaker_json(&state, user_id)).into_response())
}

/// Answers the removed scoretaker, or `null` if they weren't one.
pub async fn destroy(
    State(state): State<SharedState>,
    Path((id, user_id)): Path<(String, u64)>,
    token: Token,
) -> ApiResult {
    let caller = require_user(&token)?;
    let competition = competition(&state, &id)?;
    require_manage(&state, &competition, caller.wca_id)?;
    let removed = state.remove_scoretaker(&competition.id, user_id);
    let body = if removed {
        scoretaker_json(&state, user_id)
    } else {
        Value::Null
    };
    Ok(Json(body).into_response())
}
