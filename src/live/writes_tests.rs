//! Writing live results, through the real router.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    http::{Method, StatusCode},
    Router,
};
use serde_json::{json, Value};

use crate::route::create_router;
use crate::state::{AppState, Settings, SharedState};
use crate::v1::tests::{call, config, DELEGATE, ID, OUTSIDER};

const ROUNDS: &str = "/api/v1/competitions/DummyCubingOpen2030/live/rounds";

fn setup(delay_ms: &'static str) -> (SharedState, Router) {
    let settings = Settings::from_vars(|key| match key {
        "LIVE_RESULT_DELAY_MS" => Some(delay_ms.to_string()),
        "API_RATE_LIMIT_PER_MINUTE" => Some("0".to_string()),
        _ => None,
    })
    .unwrap();
    let state = Arc::new(
        AppState::new(config(&[
            ("GENERATED_COMPETITORS", "24"),
            ("GENERATED_EVENTS", "333:2"),
        ]))
        .with_settings(settings),
    );
    (state.clone(), create_router(state))
}

fn attempts(values: &[i64]) -> Value {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| json!({ "value": value, "attempt_number": index + 1 }))
        .collect()
}

async fn open(app: &Router, round_id: &str) -> Vec<u64> {
    let uri = format!("{}/{}/open", ROUNDS, round_id);
    let (status, body) = call(app, Method::PUT, &uri, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    round(app, round_id).await["competitors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|competitor| competitor["id"].as_u64().unwrap())
        .collect()
}

async fn round(app: &Router, round_id: &str) -> Value {
    let uri = format!("{}/{}", ROUNDS, round_id);
    call(app, Method::GET, &uri, None, None).await.1
}

async fn result(app: &Router, round_id: &str, registration_id: u64) -> Value {
    round(app, round_id).await["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["registration_id"] == registration_id)
        .unwrap()
        .clone()
}

async fn write(
    app: &Router,
    round_id: &str,
    registration_id: u64,
    values: &[i64],
) -> (StatusCode, Value) {
    let uri = format!("{}/{}", ROUNDS, round_id);
    let body = json!({ "registration_id": registration_id, "attempts": attempts(values) });
    call(app, Method::POST, &uri, Some(DELEGATE), Some(body)).await
}

#[tokio::test]
async fn a_write_is_accepted_and_replaces_the_attempts() {
    let (_, app) = setup("0");
    let competitors = open(&app, "333-r1").await;
    let id = competitors[0];

    let (status, body) = write(&app, "333-r1", id, &[1200, 1000, 1100, 1300, 1400]).await;
    assert_eq!(
        (status, body),
        (StatusCode::ACCEPTED, json!({ "status": "ok" }))
    );
    let entered = result(&app, "333-r1", id).await;
    assert_eq!(
        (entered["best"].as_i64(), entered["average"].as_i64()),
        (Some(1000), Some(1200))
    );
    assert_eq!(entered["global_pos"], 1);

    let uri = format!("{}/333-r1", ROUNDS);
    let body = json!({ "registration_id": id.to_string(), "attempts": attempts(&[900, 950]) });
    let (status, _) = call(&app, Method::PATCH, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let entered = result(&app, "333-r1", id).await;
    assert_eq!(
        entered["attempts"],
        attempts(&[900, 950]),
        "the whole list is replaced"
    );
    assert_eq!(entered["average"], 0);
}

#[tokio::test]
async fn bad_writes_get_422() {
    let (_, app) = setup("0");
    let (status, body) = write(&app, "333-r1", 1, &[1000]).await;
    assert_eq!(
        (status, body),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "status": "round is not open" })
        )
    );

    let competitors = open(&app, "333-r1").await;
    let (status, body) = write(&app, "333-r1", 1, &[1000]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({ "status": "user is not part of this round" }));

    let (status, body) = write(&app, "333-r1", competitors[0], &[1000, 0]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["status"],
        "Values cannot be 0, please omit them instead"
    );

    let uri = format!("{}/333-r1", ROUNDS);
    let (status, body) = call(&app, Method::POST, &uri, Some(DELEGATE), Some(json!({}))).await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": -4000 }))
    );

    let body = json!({ "registration_id": competitors[0], "attempts": attempts(&[1000]) });
    let (status, _) = call(&app, Method::POST, &uri, Some(OUTSIDER), Some(body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_batch_is_applied_whole_or_not_at_all() {
    let (_, app) = setup("0");
    let competitors = open(&app, "333-r1").await;
    let uri = format!("{}/333-r1/batch", ROUNDS);
    let good = |id: u64| json!({ "registration_id": id, "attempts": attempts(&[1000, 1100]) });

    let body = json!({ "results": [good(competitors[0]), good(1)] });
    let (status, error) = call(&app, Method::POST, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error,
        json!({ "status": "user is not part of this round", "registration_id": 1 })
    );
    let zero = json!({ "registration_id": competitors[1], "attempts": attempts(&[0]) });
    let body = json!({ "results": [good(competitors[0]), zero] });
    let (_, error) = call(&app, Method::POST, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(error["registration_id"], competitors[1]);
    assert_eq!(
        result(&app, "333-r1", competitors[0]).await["best"],
        0,
        "nothing applied"
    );

    let body = json!({ "results": [good(competitors[0]), good(competitors[1])] });
    let (status, _) = call(&app, Method::POST, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(result(&app, "333-r1", competitors[0]).await["best"], 1000);
    assert_eq!(result(&app, "333-r1", competitors[1]).await["best"], 1000);

    let uri = format!("{}/333-r2/batch", ROUNDS);
    let body = json!({ "results": [good(competitors[0])] });
    let (_, error) = call(&app, Method::POST, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(error, json!({ "status": "round is not open" }));
}

async fn finish(app: &Router, round_id: &str, competitors: &[u64]) {
    let uri = format!("{}/{}/batch", ROUNDS, round_id);
    let results: Vec<Value> = competitors
        .iter()
        .enumerate()
        .map(|(place, id)| {
            let value = 1000 + place as i64 * 10;
            json!({ "registration_id": id, "attempts": attempts(&[value; 5]) })
        })
        .collect();
    let (status, _) = call(
        app,
        Method::POST,
        &uri,
        Some(DELEGATE),
        Some(json!({ "results": results })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn the_next_round_waits_for_every_result_and_locked_results_take_writes() {
    let (_, app) = setup("0");
    let competitors = open(&app, "333-r1").await;
    finish(&app, "333-r1", &competitors[1..]).await;
    let uri = format!("{}/333-r2/open", ROUNDS);
    let (status, _) = call(&app, Method::PUT, &uri, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "one result is missing");

    finish(&app, "333-r1", &competitors).await;
    let proceeding: Vec<u64> = round(&app, "333-r1").await["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|result| result["advancing"] == true)
        .map(|result| result["registration_id"].as_u64().unwrap())
        .collect();
    open(&app, "333-r2").await;

    // A correction in the locked round is applied, but who proceeded stays.
    let last = *competitors.last().unwrap();
    let (status, _) = write(&app, "333-r1", last, &[500; 5]).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let corrected = result(&app, "333-r1", last).await;
    assert_eq!(corrected["average"], 500);
    assert_eq!(corrected["global_pos"], 1);
    assert_eq!(corrected["advancing"], false);
    let still: Vec<u64> = round(&app, "333-r1").await["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|result| result["advancing"] == true)
        .map(|result| result["registration_id"].as_u64().unwrap())
        .collect();
    assert_eq!(still, proceeding);
}

#[tokio::test]
async fn writes_are_applied_after_the_delay() {
    let (_, app) = setup("150");
    let competitors = open(&app, "333-r1").await;
    let (status, _) = write(&app, "333-r1", competitors[0], &[1000]).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, _) = write(&app, "333-r1", competitors[0], &[1000, 900]).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        result(&app, "333-r1", competitors[0]).await["best"],
        0,
        "not yet"
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let entered = result(&app, "333-r1", competitors[0]).await;
    assert_eq!(
        entered["attempts"],
        attempts(&[1000, 900]),
        "in the order sent"
    );
}

#[tokio::test]
async fn a_dropped_write_is_answered_but_never_applied() {
    let (_, app) = setup("0");
    let competitors = open(&app, "333-r1").await;
    let drop = format!("/dummy/competitions/{}/live/drop_next_write", ID);
    let (status, body) = call(&app, Method::POST, &drop, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["pendingDrops"], 1);

    let (status, _) = write(&app, "333-r1", competitors[0], &[1000]).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(result(&app, "333-r1", competitors[0]).await["best"], 0);
    write(&app, "333-r1", competitors[0], &[1000]).await;
    assert_eq!(result(&app, "333-r1", competitors[0]).await["best"], 1000);
}
