//! The WCA's `/api/v1` endpoints. They answer errors as `{"error": ...}`,
//! need a signed-in user unless they say otherwise, and are rate limited.

pub mod registrations;
pub mod scoretakers;

#[cfg(test)]
pub mod tests;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::{
    headers::{self, authorization::Bearer},
    TypedHeader,
};
use serde_json::{json, Value};

use crate::auth::{self, Caller};
use crate::generator::{names, Competition};
use crate::handler::can_manage_generated;
use crate::state::SharedState;

pub type Token = Option<TypedHeader<headers::Authorization<Bearer>>>;

/// An error the way `Api::V1::ApiController` renders it.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: Value,
}

impl ApiError {
    pub fn new(status: StatusCode, error: impl Into<Value>) -> Self {
        ApiError {
            status,
            body: json!({ "error": error.into() }),
        }
    }

    /// With the extra `data` registration errors carry.
    pub fn with_data(status: StatusCode, error: impl Into<Value>, data: Value) -> Self {
        ApiError {
            status,
            body: json!({ "error": error.into(), "data": data }),
        }
    }

    /// `WcaExceptions::MustLogIn`.
    pub fn must_log_in() -> Self {
        ApiError::new(StatusCode::UNAUTHORIZED, "Please log in")
    }

    /// `WcaExceptions::NotPermitted`.
    pub fn not_permitted(message: &str) -> Self {
        ApiError::new(StatusCode::FORBIDDEN, message)
    }

    /// `ActiveRecord::RecordNotFound`.
    pub fn not_found(model: &str, id: impl std::fmt::Display) -> Self {
        let id = id.to_string();
        ApiError::with_data(
            StatusCode::NOT_FOUND,
            format!("Couldn't find {} with 'id'={}", model, id),
            json!({ "model": model, "id": id }),
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub type ApiResult = Result<Response, ApiError>;

/// The signed-in user, or 401. An expired token counts as none.
pub fn require_user(token: &Token) -> Result<Caller<'_>, ApiError> {
    token
        .as_ref()
        .and_then(|token| auth::caller(token.token()))
        .ok_or_else(ApiError::must_log_in)
}

/// A generated competition. Snapshots are read-only, so the v1 endpoints
/// don't serve them.
pub fn competition(state: &SharedState, id: &str) -> Result<Arc<Competition>, ApiError> {
    state
        .generated(id)
        .ok_or_else(|| ApiError::not_found("Competition", id))
}

/// `User#can_manage_competition?`: its delegates and organizers, and WCA
/// admin teams.
pub fn can_manage(state: &SharedState, competition: &Competition, wca_id: &str) -> bool {
    can_manage_generated(state, competition, wca_id).unwrap_or(false)
}

/// `require_manage!`: 403 unless the user can manage the competition.
pub fn require_manage(
    state: &SharedState,
    competition: &Competition,
    wca_id: &str,
) -> Result<(), ApiError> {
    if can_manage(state, competition, wca_id) {
        Ok(())
    } else {
        Err(ApiError::not_permitted("Organizer privileges required"))
    }
}

/// The name of a WCA user dummy-wca knows: a fixed account or a generated
/// person.
pub fn user_name(state: &SharedState, user_id: u64) -> Option<String> {
    if let Some(account) = names::ACCOUNTS
        .iter()
        .find(|account| account.user_id == user_id)
    {
        return Some(account.name.to_string());
    }
    state.competitions().iter().find_map(|competition| {
        competition
            .persons
            .iter()
            .find(|person| person.wca_user_id == user_id)
            .map(|person| person.name.clone())
    })
}

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// Like the WCA's `ApiRateLimiting`: so many requests per minute per client
/// and controller, then 429 with an empty body until the minute is over.
pub async fn rate_limit(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    let limit = state.settings.api_rate_limit_per_minute;
    if limit == 0 {
        return next.run(request).await;
    }
    let client = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| "local".to_string(), |info| info.0.ip().to_string());
    let key = format!("{}:{}", controller(request.uri().path()), client);
    if !state.count_request(&key, limit, RATE_LIMIT_WINDOW, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    next.run(request).await
}

/// Rails counts each controller separately.
fn controller(path: &str) -> &'static str {
    if path.starts_with("/api/v1/registrations") {
        "registrations"
    } else if path.contains("/live/") {
        "live"
    } else if path.contains("/scoretakers") {
        "scoretakers"
    } else {
        "other"
    }
}
