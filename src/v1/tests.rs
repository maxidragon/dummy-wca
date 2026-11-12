//! The `/api/v1` endpoints, through the real router.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::generator::GeneratorConfig;
use crate::route::create_router;
use crate::state::{AppState, Settings};

pub const ID: &str = "DummyCubingOpen2030";
pub const DELEGATE: &str = "token-2012VAQR01";
pub const ORGANIZER: &str = "token-2015YQVI01";
pub const OUTSIDER: &str = "token-2017KEXW01";
pub const OUTSIDER_USER_ID: u64 = 9_000_003;

/// Far in the future, so competitors may still edit their registrations.
pub fn config(extra: &'static [(&'static str, &'static str)]) -> GeneratorConfig {
    GeneratorConfig::from_vars(|key| {
        if key == "GENERATED_START_DATE" {
            return Some("2030-06-01".to_string());
        }
        extra
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.to_string())
    })
    .unwrap()
}

pub fn app_with(config: GeneratorConfig, rate_limit: &str) -> Router {
    let rate_limit = rate_limit.to_string();
    let settings =
        Settings::from_vars(|key| (key == "API_RATE_LIMIT_PER_MINUTE").then(|| rate_limit.clone()))
            .unwrap();
    create_router(Arc::new(AppState::new(config).with_settings(settings)))
}

pub fn app() -> Router {
    app_with(config(&[]), "0")
}

pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }
    let request = match body {
        Some(body) => request
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, value)
}

/// A competitor with a WCA ID, and their registration as the WCIF shows it.
async fn competitor(app: &Router) -> (String, Value) {
    let uri = format!("/api/v0/competitions/{}/wcif/version/2", ID);
    let (_, wcif) = call(app, Method::GET, &uri, Some(DELEGATE), None).await;
    let person = wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|person| {
            person["wcaId"].is_string()
                && person["roles"].as_array().unwrap().is_empty()
                && person["registration"]["status"] == "accepted"
        })
        .unwrap()
        .clone();
    (
        format!("token-{}", person["wcaId"].as_str().unwrap()),
        person["registration"].clone(),
    )
}

async fn registration_events(app: &Router, registration_id: i64) -> Value {
    let uri = format!("/api/v0/competitions/{}/wcif/version/2", ID);
    let (_, wcif) = call(app, Method::GET, &uri, Some(DELEGATE), None).await;
    wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|person| person["registration"]["wcaRegistrationId"] == registration_id)
        .unwrap()["registration"]["eventIds"]
        .clone()
}

#[tokio::test]
async fn managers_replace_a_registrations_events() {
    let app = app();
    let (_, registration) = competitor(&app).await;
    let id = registration["wcaRegistrationId"].as_i64().unwrap();
    let uri = format!("/api/v1/registrations/{}", id);
    let body = json!({ "competing": { "event_ids": ["333", "pyram"] } });

    let (status, updated) = call(&app, Method::PATCH, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{}", updated);
    assert_eq!(updated["status"], "ok");
    assert_eq!(updated["registration"]["id"], id);
    assert_eq!(
        updated["registration"]["competing"]["event_ids"],
        json!(["333", "pyram"])
    );
    assert_eq!(registration_events(&app, id).await, json!(["333", "pyram"]));

    let reset = format!("/dummy/reset?id={}", ID);
    call(&app, Method::POST, &reset, None, None).await;
    assert_eq!(
        registration_events(&app, id).await,
        registration["eventIds"]
    );
}

#[tokio::test]
async fn registration_updates_check_who_and_what() {
    let app = app();
    let (competitor_token, registration) = competitor(&app).await;
    let id = registration["wcaRegistrationId"].as_i64().unwrap();
    let uri = format!("/api/v1/registrations/{}", id);
    let events = json!({ "competing": { "event_ids": ["333"] } });
    let patch = |token: Option<String>, body: Value| {
        let app = app.clone();
        let uri = uri.clone();
        async move { call(&app, Method::PATCH, &uri, token.as_deref(), Some(body)).await }
    };

    let (status, body) = patch(None, events.clone()).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::UNAUTHORIZED, &json!("Please log in"))
    );

    let (status, body) = patch(Some(OUTSIDER.to_string()), events.clone()).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::UNAUTHORIZED, &json!(-2003))
    );

    // A competitor may change their own events before the competition starts.
    let (status, _) = patch(Some(competitor_token), events.clone()).await;
    assert_eq!(status, StatusCode::OK);

    // A token without manage_registrations may not, even a manager's.
    let scoped = crate::auth::access_token("2012VAQR01", &["public"], i64::MAX);
    let (status, body) = patch(Some(scoped), events.clone()).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::FORBIDDEN, &json!(-2003))
    );

    let (status, body) = patch(
        Some(DELEGATE.to_string()),
        json!({ "competing": { "event_ids": ["333", "777"] } }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({ "error": -4004, "data": ["777"] }));

    let (status, body) = patch(Some(DELEGATE.to_string()), json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "You need to supply at least one lane");

    let (status, body) = call(
        &app,
        Method::PATCH,
        "/api/v1/registrations/1",
        Some(DELEGATE),
        Some(events),
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::NOT_FOUND, &json!(-3000))
    );
}

#[tokio::test]
async fn managers_are_held_to_the_events_limit() {
    let app = app_with(
        config(&[("GENERATED_EVENTS_PER_REGISTRATION_LIMIT", "2")]),
        "0",
    );
    let (_, registration) = competitor(&app).await;
    let uri = format!(
        "/api/v1/registrations/{}",
        registration["wcaRegistrationId"]
    );
    let three = json!({ "competing": { "event_ids": ["333", "222", "pyram"] } });
    let (status, body) = call(&app, Method::PATCH, &uri, Some(DELEGATE), Some(three)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], -4004);
    assert_eq!(body["data"]["count"], 2);
}

#[tokio::test]
async fn managers_add_and_remove_scoretakers() {
    let app = app();
    let uri = format!("/api/v1/competitions/{}/scoretakers", ID);
    let one = format!("{}/{}", uri, OUTSIDER_USER_ID);

    assert_eq!(
        call(&app, Method::GET, &uri, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, list) = call(&app, Method::GET, &uri, Some(OUTSIDER), None).await;
    assert_eq!((status, list), (StatusCode::OK, json!([])));

    let add = json!({ "user_id": OUTSIDER_USER_ID });
    let (status, body) = call(&app, Method::POST, &uri, Some(OUTSIDER), Some(add.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "Organizer privileges required");

    let (status, added) = call(&app, Method::POST, &uri, Some(ORGANIZER), Some(add.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        added,
        json!({ "user_id": OUTSIDER_USER_ID, "name": "Otis Kexwold" })
    );
    call(&app, Method::POST, &uri, Some(ORGANIZER), Some(add)).await;
    let (_, list) = call(&app, Method::GET, &uri, Some(OUTSIDER), None).await;
    assert_eq!(list, json!([added]), "added once");

    let (status, body) = call(
        &app,
        Method::POST,
        &uri,
        Some(DELEGATE),
        Some(json!({ "user_id": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{}", body);

    assert_eq!(
        call(&app, Method::DELETE, &one, Some(OUTSIDER), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, removed) = call(&app, Method::DELETE, &one, Some(DELEGATE), None).await;
    assert_eq!((status, removed), (StatusCode::OK, added));
    let (_, removed) = call(&app, Method::DELETE, &one, Some(DELEGATE), None).await;
    assert_eq!(removed, Value::Null);

    let unknown = "/api/v1/competitions/NoSuchCompetition2030/scoretakers";
    assert_eq!(
        call(&app, Method::GET, unknown, Some(DELEGATE), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn v1_requests_are_rate_limited_per_controller() {
    let app = app_with(config(&[]), "3");
    let uri = format!("/api/v1/competitions/{}/scoretakers", ID);
    for _ in 0..3 {
        let (status, _) = call(&app, Method::GET, &uri, Some(DELEGATE), None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, body) = call(&app, Method::GET, &uri, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body, json!(""));

    // Other controllers and the v0 API count separately.
    let (status, _) = call(
        &app,
        Method::PATCH,
        "/api/v1/registrations/1",
        Some(DELEGATE),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::GET, "/api/v0/me", Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn created_competitions_get_their_own_registration_ids() {
    let app = app();
    let request = Request::builder()
        .method(Method::POST)
        .uri("/dummy/competitions")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "name=Dummy+Cubing+Open&start_date=2031-06-01&event_333=1",
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::SEE_OTHER
    );
    let ids = |wcif: &Value| -> Vec<i64> {
        wcif["persons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|person| {
                person["registration"]["wcaRegistrationId"]
                    .as_i64()
                    .unwrap()
            })
            .collect()
    };
    let uri = |id: &str| format!("/api/v0/competitions/{}/wcif/public", id);
    let (_, first) = call(&app, Method::GET, &uri(ID), None, None).await;
    let (_, second) = call(&app, Method::GET, &uri("DummyCubingOpen2031"), None, None).await;
    let first = ids(&first);
    assert!(ids(&second).iter().all(|id| !first.contains(id)));
}
