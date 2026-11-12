//! The live results endpoints, through the real router.

use std::sync::Arc;

use axum::{
    http::{Method, StatusCode},
    Router,
};
use serde_json::{json, Value};

use super::model::{Attempt, LiveResult};
use super::{definition, recompute};
use crate::route::create_router;
use crate::state::{AppState, SharedState};
use crate::v1::tests::{call, config, DELEGATE, ID, OUTSIDER, OUTSIDER_USER_ID};

const ROUNDS: &str = "/api/v1/competitions/DummyCubingOpen2030/live/rounds";

fn setup() -> (SharedState, Router) {
    let state = Arc::new(AppState::new(config(&[
        ("GENERATED_COMPETITORS", "24"),
        ("GENERATED_EVENTS", "333:2"),
    ])));
    let app = create_router(state.clone());
    (state, app)
}

/// Gives everyone in the round a complete result, the first fastest.
fn finish_round(state: &SharedState, round_id: &str) {
    let competition = state.generated(ID).unwrap();
    let definition = definition(state, &competition);
    let index = definition.round(round_id).unwrap();
    state.live.with(ID, |results| {
        let round_results = results.get_mut(round_id).unwrap();
        for (place, result) in round_results.iter_mut().enumerate() {
            let value = 1000 + place as i64 * 10;
            result.attempts = (1..=5)
                .map(|attempt_number| Attempt {
                    attempt_number,
                    value,
                })
                .collect();
            result.best = value;
            result.average = value;
        }
        recompute(&definition, index, round_results, false);
    });
}

fn results(state: &SharedState, round_id: &str) -> Vec<LiveResult> {
    state.live.with(ID, |results| {
        results.get(round_id).cloned().unwrap_or_default()
    })
}

async fn round_states(app: &Router) -> Vec<String> {
    let (status, body) = call(app, Method::GET, ROUNDS, None, None).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    body["rounds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|round| round["state"].as_str().unwrap().to_string())
        .collect()
}

async fn put(app: &Router, uri: &str, token: &str) -> (StatusCode, Value) {
    call(app, Method::PUT, uri, Some(token), None).await
}

#[tokio::test]
async fn rounds_open_lock_and_close_like_the_wca() {
    let (state, app) = setup();
    assert_eq!(round_states(&app).await, ["ready", "pending"]);

    let (status, body) = put(&app, &format!("{}/333-r2/open", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["status"],
        "score taking is not finished in the previous round"
    );

    let (status, opened) = put(&app, &format!("{}/333-r1/open", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::OK, "{}", opened);
    assert_eq!(opened["state"], "open");
    assert_eq!(opened["locked_rows"], 0);
    let competitors = opened["created_rows"].as_u64().unwrap();
    assert!(competitors >= 16);
    let (status, body) = put(&app, &format!("{}/333-r1/open", ROUNDS), DELEGATE).await;
    assert_eq!(
        (status, &body["status"]),
        (StatusCode::BAD_REQUEST, &json!("round already open"))
    );
    assert_eq!(round_states(&app).await, ["open", "pending"]);

    finish_round(&state, "333-r1");
    assert_eq!(round_states(&app).await, ["open", "ready"]);

    let (status, opened) = put(&app, &format!("{}/333-r2/open", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::OK, "{}", opened);
    assert_eq!(opened["locked_rows"], competitors);
    let proceeding = results(&state, "333-r1")
        .iter()
        .filter(|result| result.advancing)
        .count();
    assert_eq!(opened["created_rows"], proceeding);
    assert_eq!(round_states(&app).await, ["locked", "open"]);

    let close = format!("{}/333-r2/close", ROUNDS);
    let (status, closed) = call(&app, Method::DELETE, &close, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK, "{}", closed);
    assert_eq!(closed["deleted_count"], proceeding);
    assert_eq!(
        round_states(&app).await,
        ["open", "ready"],
        "round 1 unlocked"
    );

    let close = format!("{}/333-r1/close", ROUNDS);
    let (status, body) = call(&app, Method::DELETE, &close, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["status"], "round has results entered");

    let (status, cleared) = put(&app, &format!("{}/333-r1/clear", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["recreated_rows"], competitors);
    let (status, _) = call(&app, Method::DELETE, &close, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(round_states(&app).await, ["ready", "pending"]);
}

#[tokio::test]
async fn round_results_are_public_and_carry_a_state_hash() {
    let (state, app) = setup();
    put(&app, &format!("{}/333-r1/open", ROUNDS), DELEGATE).await;
    let uri = format!("{}/333-r1", ROUNDS);
    let (status, before) = call(&app, Method::GET, &uri, None, None).await;
    assert_eq!(status, StatusCode::OK, "{}", before);
    assert_eq!(before["id"], "333-r1");
    assert_eq!(before["format"], "a");
    assert!(before["round_id"].is_number());
    let competitor = &before["competitors"][0];
    for key in [
        "id",
        "user_id",
        "registrant_id",
        "name",
        "country_iso2",
        "wca_id",
    ] {
        assert!(competitor.get(key).is_some(), "competitor has {}", key);
    }
    assert_eq!(before["results"][0]["attempts"], json!([]));
    assert_eq!(before["completed_competitors"], 0);

    finish_round(&state, "333-r1");
    let (_, after) = call(&app, Method::GET, &uri, None, None).await;
    let result = &after["results"][0];
    assert_eq!(
        result["attempts"][0],
        json!({ "value": 1000, "attempt_number": 1 })
    );
    assert_eq!(result["best"], 1000);
    assert_eq!(result["average"], 1000);
    assert_eq!(result["global_pos"], 1);
    assert_eq!(result["advancing"], true);
    assert!(result["advancing_questionable"].is_boolean());
    assert_ne!(before["state_hash"], after["state_hash"]);
}

#[tokio::test]
async fn only_managers_and_scoretakers_may_scoretake() {
    let (_, app) = setup();
    let open = format!("{}/333-r1/open", ROUNDS);
    let (status, body) = call(&app, Method::PUT, &open, None, None).await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::UNAUTHORIZED, &json!("Please log in"))
    );
    let (status, body) = put(&app, &open, OUTSIDER).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, json!({ "error": "Score taking privileges required" }));

    let scoretakers = format!("/api/v1/competitions/{}/scoretakers", ID);
    let add = json!({ "user_id": OUTSIDER_USER_ID });
    call(&app, Method::POST, &scoretakers, Some(DELEGATE), Some(add)).await;
    assert_eq!(put(&app, &open, OUTSIDER).await.0, StatusCode::OK);

    let (status, _) = put(&app, &format!("{}/333-r9/open", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn other_scoretaking_software_gets_403() {
    let state = Arc::new(AppState::new(config(&[
        ("GENERATED_EVENTS", "333:1"),
        ("GENERATED_SCORETAKING_SOFTWARE", "wca_live"),
    ])));
    let app = create_router(state);
    let (status, body) = call(&app, Method::GET, ROUNDS, None, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"],
        "Scoretaking software needs to be set to Internal"
    );
    let (status, _) = put(&app, &format!("{}/333-r1/open", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Round results stay public.
    let uri = format!("{}/333-r1", ROUNDS);
    assert_eq!(
        call(&app, Method::GET, &uri, None, None).await.0,
        StatusCode::OK
    );
}

/// Round 1 finished and round 2 opened: returns who proceeded, best first.
async fn second_round_open(state: &SharedState, app: &Router) -> Vec<u64> {
    put(app, &format!("{}/333-r1/open", ROUNDS), DELEGATE).await;
    finish_round(state, "333-r1");
    put(app, &format!("{}/333-r2/open", ROUNDS), DELEGATE).await;
    results(state, "333-r1")
        .iter()
        .filter(|result| result.advancing)
        .map(|result| result.registration_id)
        .collect()
}

#[tokio::test]
async fn quitting_checks_who_should_proceed_instead() {
    let (state, app) = setup();
    let proceeding = second_round_open(&state, &app).await;
    let winner = proceeding[0];
    let round_one = results(&state, "333-r1");
    let next = round_one
        .iter()
        .find(|result| !result.advancing)
        .unwrap()
        .registration_id;

    let uri = format!("{}/333-r2/next_if_quit?registration_id={}", ROUNDS, winner);
    let (status, body) = call(&app, Method::GET, &uri, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["next_advancing"][0]["id"], next);
    assert_eq!(body["next_advancing"].as_array().unwrap().len(), 1);

    let quit = format!("{}/333-r2/{}", ROUNDS, winner);
    let wrong = json!({ "advancing_ids": [winner] });
    let (status, body) = call(&app, Method::DELETE, &quit, Some(DELEGATE), Some(wrong)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["should_advance"][0]["registration_id"], next);

    let right = format!("{}?advancing_ids[]={}", quit, next);
    let (status, body) = call(&app, Method::DELETE, &right, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["quit"], 1);

    let round_two: Vec<u64> = results(&state, "333-r2")
        .iter()
        .map(|result| result.registration_id)
        .collect();
    assert!(!round_two.contains(&winner));
    assert!(round_two.contains(&next));
    let round_one = results(&state, "333-r1");
    let quitter = round_one
        .iter()
        .find(|r| r.registration_id == winner)
        .unwrap();
    assert!(quitter.quit && !quitter.advancing);
    assert!(
        round_one
            .iter()
            .find(|r| r.registration_id == next)
            .unwrap()
            .advancing
    );

    // Quitting from a first round can't name anyone to proceed.
    let uri = format!("{}/333-r1/{}?advancing_ids[]=1", ROUNDS, next);
    let (status, body) = call(&app, Method::DELETE, &uri, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["status"], "Can't advance next for first rounds");
    let uri = format!("{}/333-r1/{}", ROUNDS, next);
    let (status, body) = call(&app, Method::DELETE, &uri, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["status"], "Cannot quit competitor with results");
}

#[tokio::test]
async fn bulk_quit_and_adding_back() {
    let (state, app) = setup();
    let proceeding = second_round_open(&state, &app).await;
    let uri = format!("{}/333-r2/bulk_quit", ROUNDS);
    let body = json!({ "registration_ids": [proceeding[1], proceeding[2]] });
    let (status, quit) = call(&app, Method::DELETE, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{}", quit);
    assert_eq!(quit["quit"], 2);
    assert_eq!(results(&state, "333-r2").len(), proceeding.len() - 2);
    let (status, _) = call(&app, Method::DELETE, &uri, Some(DELEGATE), Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let addable = format!("{}/333-r2/addable_competitors", ROUNDS);
    let (status, body) = call(&app, Method::GET, &addable, Some(DELEGATE), None).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<u64> = body["registrations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|registration| registration["id"].as_u64().unwrap())
        .collect();
    assert!(ids.contains(&proceeding[1]), "everyone from round 1");
    assert_eq!(body["colinked_status"], json!([]));

    let add = format!("{}/333-r2/{}", ROUNDS, proceeding[1]);
    let (status, added) = put(&app, &add, DELEGATE).await;
    assert_eq!(status, StatusCode::OK, "{}", added);
    assert_eq!(added["competitor"]["id"], proceeding[1]);
    assert_eq!(results(&state, "333-r2").len(), proceeding.len() - 1);

    let (status, _) = put(&app, &format!("{}/333-r2/1", ROUNDS), DELEGATE).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no such registration");
}

#[tokio::test]
async fn clearing_a_competitor_removes_their_attempts() {
    let (state, app) = setup();
    put(&app, &format!("{}/333-r1/open", ROUNDS), DELEGATE).await;
    finish_round(&state, "333-r1");
    let first = results(&state, "333-r1")[0].registration_id;
    let uri = format!("{}/333-r1/{}/clear", ROUNDS, first);
    let (status, body) = put(&app, &uri, DELEGATE).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["deleted_attempts"], 5);
    let cleared = results(&state, "333-r1")
        .into_iter()
        .find(|result| result.registration_id == first)
        .unwrap();
    assert!(cleared.attempts.is_empty() && cleared.best == 0 && !cleared.advancing);
    assert_eq!(cleared.global_pos, None);

    let reset = format!("/dummy/reset?id={}", ID);
    call(&app, Method::POST, &reset, None, None).await;
    assert_eq!(round_states(&app).await, ["ready", "pending"]);
}
