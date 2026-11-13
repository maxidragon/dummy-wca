//! End-to-end tests of the routes FKMTime calls, through the real router.

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
use crate::utils::read_json_file;

const ID: &str = "DummyCubingOpen2026";
const DELEGATE: &str = "token-2012VAQR01";
const ORGANIZER: &str = "token-2015YQVI01";
const OUTSIDER: &str = "token-2017KEXW01";
const ADMIN: &str = "token-2011ULQA01";

fn pinned(date: &str) -> GeneratorConfig {
    let date = date.to_string();
    GeneratorConfig::from_vars(move |key| (key == "GENERATED_START_DATE").then(|| date.clone()))
        .unwrap()
}

fn app() -> Router {
    create_router(Arc::new(AppState::new(pinned("2026-10-26"))))
}

async fn call(
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

async fn get(app: &Router, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    call(app, Method::GET, uri, token, None).await
}

#[tokio::test]
async fn discovery_names_the_competition_and_accounts() {
    let (status, body) = get(&app(), "/dummy/generated", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], ID);
    assert_eq!(body["startDate"], "2026-10-26");
    assert_eq!(body["endDate"], "2026-10-27");
    assert_eq!(body["accounts"]["delegate"]["code"], "code-2012VAQR01");
    assert_eq!(body["accounts"]["delegate"]["role"], "delegate");
    assert_eq!(body["accounts"]["organizer"]["role"], "organizer");
    assert_eq!(body["accounts"]["outsider"]["role"], Value::Null);
    assert_eq!(body["accounts"]["admin"]["role"], "wst");
}

fn started_at(now: &str) -> Router {
    create_router(Arc::new(AppState::started_at(
        GeneratorConfig::default(),
        now.parse().unwrap(),
    )))
}

#[tokio::test]
async fn today_is_the_venue_date_at_startup() {
    // 22:30 UTC on 31 December is 23:30 in Warsaw: still 2026.
    let (_, body) = get(
        &started_at("2026-12-31T22:30:00Z"),
        "/dummy/generated",
        None,
    )
    .await;
    assert_eq!(body["id"], "DummyCubingOpen2026");
    assert_eq!(body["startDate"], "2026-12-31");
    assert_eq!(body["startDatePinned"], false);
    assert_eq!(body["serverStartedAt"], "2026-12-31T22:30:00Z");

    // An hour later it is already 1 January 2027 in Warsaw.
    let app = started_at("2026-12-31T23:30:00Z");
    let (_, body) = get(&app, "/dummy/generated", None).await;
    assert_eq!(body["id"], "DummyCubingOpen2027");
    assert_eq!(body["startDate"], "2027-01-01");
    let uri = "/api/v0/competitions/DummyCubingOpen2026/wcif/version/2";
    assert_eq!(get(&app, uri, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_competition_does_not_move_while_the_server_runs() {
    // The date is resolved once: a server started on New Year's Eve keeps
    // serving the 2026 competition after midnight, until it restarts.
    let state = Arc::new(AppState::started_at(
        GeneratorConfig::default(),
        "2026-12-31T22:30:00Z".parse().unwrap(),
    ));
    let before = state.competition();
    let app = create_router(state.clone());
    let (status, wcif) = get(
        &app,
        "/api/v0/competitions/DummyCubingOpen2026/wcif/version/2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(wcif["schedule"]["startDate"], "2026-12-31");
    assert!(Arc::ptr_eq(&before, &state.competition()));
}

#[tokio::test]
async fn a_pinned_date_ignores_the_clock() {
    let state = AppState::started_at(
        pinned("2027-03-20"),
        "2026-12-31T23:30:00Z".parse().unwrap(),
    );
    let app = create_router(Arc::new(state));
    let (_, body) = get(&app, "/dummy/generated", None).await;
    assert_eq!(body["id"], "DummyCubingOpen2027");
    assert_eq!(body["startDate"], "2027-03-20");
    assert_eq!(body["startDatePinned"], true);
}

#[tokio::test]
async fn sign_in_lists_the_competition_for_its_managers() {
    let app = app();
    let (status, token) = call(
        &app,
        Method::POST,
        "/oauth/token",
        None,
        Some(json!({
            "code": "code-2012VAQR01",
            "client_id": "example-application-id",
            "client_secret": "example-secret",
            "redirect_uri": "http://localhost/auth/login",
            "grant_type": "authorization_code",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let signed_in = token["access_token"].as_str().unwrap();

    let (status, me) = get(&app, "/api/v0/me", Some(signed_in)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["me"]["id"], 9_000_001);
    assert_eq!(me["me"]["delegate_status"], "delegate");
    assert!(me["me"]["avatar"]["thumb_url"].is_string());

    let uri =
        "/api/v0/competitions?managed_by_me=true&start=2026-10-19T12:00:00.000Z&sort=start_date";
    let (status, listed) = get(&app, uri, Some(signed_in)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["id"], ID);
    assert_eq!(listed[0]["delegates"][0]["id"], 9_000_001);

    let (_, listed) = get(&app, uri, Some(ORGANIZER)).await;
    assert_eq!(listed[0]["id"], ID);
    let delegates = listed[0]["delegates"].as_array().unwrap();
    assert!(delegates.iter().all(|delegate| delegate["id"] != 9_000_002));

    let (_, listed) = get(&app, uri, Some(OUTSIDER)).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn start_and_end_filter_like_the_wca() {
    let app = app();
    let listed = |query: &'static str| {
        let app = app.clone();
        async move {
            get(
                &app,
                &format!("/api/v0/competitions?managed_by_me=true&{}", query),
                Some(DELEGATE),
            )
            .await
            .1
        }
    };
    // Ends on 27 Oct: still listed from a start of 27 Oct, not from 28 Oct.
    assert_eq!(
        listed("start=2026-10-27").await.as_array().unwrap().len(),
        1
    );
    assert_eq!(listed("start=2026-10-28T00:00:00.000Z").await, json!([]));
    assert_eq!(listed("end=2026-10-26").await.as_array().unwrap().len(), 1);
    assert_eq!(listed("end=2026-10-25").await, json!([]));
}

#[tokio::test]
async fn snapshot_accounts_are_unchanged() {
    let app = app();
    let (status, listed) = get(
        &app,
        "/api/v0/competitions?managed_by_me=true&start=2026-10-19",
        Some("token-2013KOSK01"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listed,
        read_json_file("data/users/2013KOSK01/manageable-competitions.json").unwrap()
    );

    let (status, wcif) = get(&app, "/api/v0/competitions/SLSFinal2024/wcif/public", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        wcif,
        read_json_file("data/competitions/SLSFinal2024/wcif.json").unwrap()
    );

    let (status, wcif) = get(
        &app,
        "/api/v0/competitions/WLSMarzec2026/wcif/version/2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        wcif,
        read_json_file("data/competitions/WLSMarzec2026/wcif_v2.json").unwrap()
    );

    let (status, _) = get(
        &app,
        "/api/v0/competitions/NoSuchCompetition2099/wcif/version/2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn private_wcif_needs_rights() {
    let app = app();
    let uri = format!("/api/v0/competitions/{}/wcif/version/2", ID);

    let (status, public) = get(&app, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public["formatVersion"], "2.1.1");
    assert!(public["persons"][0].get("birthdate").is_none());

    for token in [DELEGATE, ORGANIZER, ADMIN, "token-2022GALA01"] {
        let (status, private) = get(&app, &uri, Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{}", token);
        assert!(private["persons"][0]["birthdate"].is_string());
    }
    assert_eq!(
        get(&app, &uri, Some(OUTSIDER)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&app, &uri, Some("token-2099NONE01")).await.0,
        StatusCode::UNAUTHORIZED
    );

    let v1 = format!("/api/v0/competitions/{}/wcif", ID);
    assert_eq!(get(&app, &v1, None).await.0, StatusCode::UNAUTHORIZED);
    let (status, private) = get(&app, &v1, Some(DELEGATE)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(private["formatVersion"], "1.1");

    let (status, public) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif/public", ID),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public["formatVersion"], "1.1");

    let (status, info) = get(&app, &format!("/api/v0/competitions/{}", ID), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["delegates"][0]["wca_id"], "2012VAQR01");
    assert_eq!(info["organizers"][0]["id"], 9_000_002);

    assert_eq!(
        get(
            &app,
            "/api/v0/competitions/DummyCubingOpen2025/wcif/version/2",
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

/// A competitor and a group activity from the private WCIF.
async fn competitor_and_group(app: &Router) -> (Value, i64) {
    let (_, wcif) = get(
        app,
        &format!("/api/v0/competitions/{}/wcif/version/2", ID),
        Some(DELEGATE),
    )
    .await;
    let person = wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|person| {
            person["registrantId"].is_i64() && !person["assignments"].as_array().unwrap().is_empty()
        })
        .unwrap()
        .clone();
    let group = wcif["schedule"]["venues"][0]["rooms"][0]["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|activity| activity["childActivities"][0]["id"].as_i64())
        .unwrap();
    (person, group)
}

#[tokio::test]
async fn patch_saves_assignments_until_reset() {
    let app = app();
    let (mut person, group) = competitor_and_group(&app).await;
    let registrant_id = person["registrantId"].clone();
    person["assignments"] =
        json!([{ "activityId": group, "stationNumber": null, "assignmentCode": "competitor" }]);
    let body = json!({ "id": ID, "formatVersion": "2.1.1", "persons": [person] });
    let uri = format!("/api/v0/competitions/{}/wcif", ID);

    assert_eq!(
        call(&app, Method::PATCH, &uri, None, Some(body.clone()))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &uri,
            Some(OUTSIDER),
            Some(body.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, saved) = call(&app, Method::PATCH, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["status"], "Successfully saved WCIF");

    let find = |wcif: &Value| {
        wcif["persons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|person| person["registrantId"] == registrant_id)
            .unwrap()["assignments"]
            .clone()
    };
    let expected =
        json!([{ "activityId": group, "stationNumber": null, "assignmentCode": "competitor" }]);
    for uri in [
        format!("/api/v0/competitions/{}/wcif/version/2", ID),
        format!("/api/v0/competitions/{}/wcif", ID),
    ] {
        let (_, wcif) = get(&app, &uri, Some(DELEGATE)).await;
        assert_eq!(find(&wcif), expected, "{}", uri);
    }
    let (_, public) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif/version/2", ID),
        None,
    )
    .await;
    assert_eq!(find(&public), expected);
    assert_eq!(
        get(&app, "/dummy/generated", None).await.1["patchedPersons"],
        1
    );

    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/dummy/reset?id={}", ID),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, wcif) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif/version/2", ID),
        Some(DELEGATE),
    )
    .await;
    assert_ne!(find(&wcif), expected);
}

async fn token_request(app: &Router, form: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "client_id=example-application-id&client_secret=example-secret&{}",
                    form
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn sign_in_accepts_a_form_and_says_when_the_token_expires() {
    let app = app();
    let (status, body) = token_request(
        &app,
        "grant_type=authorization_code&code=code-2012VAQR01\
         &redirect_uri=http%3A%2F%2Flocalhost%2Fcallback",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["expires_in"], 7200);
    assert!(body["refresh_token"].is_string());
    let token = body["access_token"].as_str().unwrap();
    let (status, me) = get(&app, "/api/v0/me", Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["me"]["wca_id"], "2012VAQR01");
}

#[tokio::test]
async fn a_refresh_token_gives_a_new_token_once() {
    let app = app();
    let (_, first) = token_request(
        &app,
        "grant_type=authorization_code&code=code-2012VAQR01-public,manage_registrations",
    )
    .await;
    assert_eq!(first["scope"], "public manage_registrations");
    let refresh = first["refresh_token"].as_str().unwrap();

    let form = format!("grant_type=refresh_token&refresh_token={}", refresh);
    let (status, second) = token_request(&app, &form).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["scope"], "public manage_registrations");
    assert_ne!(second["refresh_token"], first["refresh_token"]);
    let token = second["access_token"].as_str().unwrap();
    assert_eq!(get(&app, "/api/v0/me", Some(token)).await.0, StatusCode::OK);

    let (status, body) = token_request(&app, &form).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_grant");
}

#[tokio::test]
async fn tokens_expire_after_the_configured_lifetime() {
    let settings =
        Settings::from_vars(|key| (key == "TOKEN_LIFETIME_SECONDS").then(|| "60".to_string()))
            .unwrap();
    let app = create_router(Arc::new(
        AppState::new(pinned("2026-10-26")).with_settings(settings),
    ));
    let (_, body) = token_request(&app, "grant_type=authorization_code&code=code-2012VAQR01").await;
    assert_eq!(body["expires_in"], 60);

    let expired = crate::auth::access_token("2012VAQR01", &["public"], 1);
    assert_eq!(
        get(&app, "/api/v0/me", Some(&expired)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(Settings::from_vars(|_| Some("0".to_string())).is_err());
}

#[tokio::test]
async fn check_says_what_a_patch_would_do_and_saves_nothing() {
    let app = app();
    let (_, before) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif", ID),
        Some(DELEGATE),
    )
    .await;
    let body = json!({"id": ID, "extensions": [{"id": "example.Extension"}]});

    let (status, _) = call(
        &app,
        Method::PUT,
        "/api/v0/competitions/wcif/check",
        Some(DELEGATE),
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif", ID),
        Some(DELEGATE),
    )
    .await;
    assert_eq!(before, after);

    let (status, _) = call(
        &app,
        Method::PUT,
        "/api/v0/competitions/wcif/check",
        Some(OUTSIDER),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::PUT,
        "/api/v0/competitions/wcif/check",
        Some(DELEGATE),
        Some(json!({"id": ID, "persons": "nobody"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn patch_saves_schedule_with_assignments_to_its_new_groups() {
    let app = app();
    let uri = format!("/api/v0/competitions/{}/wcif", ID);
    let (_, wcif) = get(&app, &uri, Some(DELEGATE)).await;
    let mut schedule = wcif["schedule"].clone();
    let new_group = 987_654;
    schedule["venues"][0]["rooms"][0]["activities"][0]["childActivities"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": new_group, "name": "New group", "activityCode": "333-r1-g9"}));
    let user_id = wcif["persons"][0]["wcaUserId"].clone();

    let (status, _) = call(
        &app,
        Method::PATCH,
        &uri,
        Some(DELEGATE),
        Some(json!({
            "schedule": schedule,
            "extensions": [{"id": "example.Extension"}],
            "persons": [{
                "wcaUserId": user_id,
                "assignments": [{
                    "activityId": new_group,
                    "assignmentCode": "competitor",
                    "stationNumber": null,
                }],
            }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, saved) = get(&app, &uri, Some(DELEGATE)).await;
    assert_eq!(saved["schedule"], schedule);
    assert_eq!(saved["extensions"][0]["id"], "example.Extension");
    assert_eq!(
        saved["persons"][0]["assignments"][0]["activityId"],
        new_group
    );

    // A later PATCH of persons alone still knows the group.
    let (status, _) = call(
        &app,
        Method::PATCH,
        &uri,
        Some(DELEGATE),
        Some(json!({"persons": [{
            "wcaUserId": user_id,
            "assignments": [{"activityId": new_group, "assignmentCode": "staff-judge"}],
        }]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(&app, Method::POST, "/dummy/reset", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, reset) = get(&app, &uri, Some(DELEGATE)).await;
    assert_eq!(reset["schedule"], wcif["schedule"]);
}

#[tokio::test]
async fn patch_rejects_bad_bodies() {
    let app = app();
    let (person, group) = competitor_and_group(&app).await;
    let uri = format!("/api/v0/competitions/{}/wcif", ID);
    let user_id = person["wcaUserId"].clone();
    let bad_bodies = [
        json!([]),
        json!({ "id": "OtherCompetition2026", "persons": [] }),
        json!({ "id": ID, "schedule": {} }),
        json!({ "id": ID, "persons": {} }),
        json!({ "id": ID, "persons": ["not a person"] }),
        json!({ "id": ID, "persons": [{ "wcaUserId": user_id, "assignments": [{ "activityId": 99999, "assignmentCode": "competitor" }] }] }),
        json!({ "id": ID, "persons": [{ "wcaUserId": user_id, "assignments": [{ "activityId": group }] }] }),
    ];
    for body in bad_bodies {
        let (status, error) = call(
            &app,
            Method::PATCH,
            &uri,
            Some(DELEGATE),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{} -> {}", body, error);
    }
    assert_eq!(
        get(&app, "/dummy/generated", None).await.1["patchedPersons"],
        0
    );
}

#[tokio::test]
async fn patch_matches_persons_by_wca_user_id_like_the_wca() {
    let app = app();
    let uri = format!("/api/v0/competitions/{}/wcif", ID);
    let wcif_uri = format!("/api/v0/competitions/{}/wcif/version/2", ID);
    let (_, wcif) = get(&app, &wcif_uri, Some(DELEGATE)).await;
    let mut persons = wcif["persons"].as_array().unwrap().clone();
    // Pending and deleted registrations have no registrantId
    assert!(persons.iter().any(|p| p["registrantId"].is_null()));
    let (_, group) = competitor_and_group(&app).await;
    let index = persons
        .iter()
        .position(|p| p["registrantId"].is_i64())
        .unwrap();
    let user_id = persons[index]["wcaUserId"].clone();
    persons[index]["assignments"] =
        json!([{ "activityId": group, "stationNumber": null, "assignmentCode": "competitor" }]);
    // Fields the WCA doesn't save from a PATCH are ignored
    persons[index]["registrantId"] = Value::Null;
    persons[index]["name"] = json!("Renamed");
    // Someone with no registration is skipped, not an error
    persons.push(json!({ "wcaUserId": 1, "registrantId": null, "name": "Stranger" }));
    persons.push(json!({ "name": "No user id" }));

    let body = json!({ "id": ID, "formatVersion": "2.1.1", "persons": persons });
    let (status, saved) = call(&app, Method::PATCH, &uri, Some(DELEGATE), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{}", saved);

    let (_, wcif) = get(&app, &wcif_uri, Some(DELEGATE)).await;
    let person = wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["wcaUserId"] == user_id)
        .unwrap()
        .clone();
    assert_eq!(person["assignments"][0]["activityId"], group);
    assert!(person["registrantId"].is_i64());
    assert_ne!(person["name"], "Renamed");
    assert!(wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["name"] != "Stranger"));
}

#[tokio::test]
async fn users_and_persons_include_generated_people() {
    let app = app();
    let (status, user) = get(&app, "/api/v0/users/2017KEXW01", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["user"]["id"], 9_000_003);
    assert_eq!(
        get(&app, "/api/v0/users/2099NONE01", None).await.0,
        StatusCode::NOT_FOUND
    );

    // Search ignores case.
    let (_, found) = get(&app, "/api/v0/persons?q=VAQR", None).await;
    assert_eq!(found[0]["person"]["wca_id"], "2012VAQR01");
    let (_, found) = get(&app, "/api/v0/persons?q=kosk", None).await;
    assert_eq!(found[0]["person"]["wca_id"], "2013KOSK01");

    let (_, wcif) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif/version/2", ID),
        None,
    )
    .await;
    let generated = wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|person| {
            person["wcaId"].is_string() && person["wcaUserId"].as_u64() > Some(9_100_000)
        })
        .unwrap();
    let wca_id = generated["wcaId"].as_str().unwrap();
    let (status, person) = get(
        &app,
        &format!("/api/v0/persons/{}", wca_id.to_lowercase()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(person["person"]["name"], generated["name"]);
    assert!(person["person"]["country"]["iso2"].is_string());
    let (status, user) = get(&app, &format!("/api/v0/users/{}", wca_id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["user"]["id"], generated["wcaUserId"]);

    assert_eq!(
        get(&app, "/api/v0/persons/2099NONE01", None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn authorize_page_offers_the_generated_accounts() {
    let uri = "/oauth/authorize?redirect_uri=http://localhost/auth/login&scope=public&response_type=code&client_id=example-application-id";
    let (status, page) = get(&app(), uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let page = page.as_str().unwrap();
    assert!(page.contains("<title>Dummy WCA - authorize</title>"));
    assert!(page.contains("login('2012VAQR01')"));
    assert!(page.contains("login('2013ROGA02')"));
    assert!(
        page.contains("code-${wcaId}-public`"),
        "the code carries the scope"
    );
}

#[tokio::test]
async fn authorize_page_passes_requested_scopes_and_refuses_unknown_ones() {
    let uri = "/oauth/authorize?redirect_uri=http://localhost/auth/login&scope=public+manage_registrations&response_type=code&client_id=example-application-id";
    let (status, page) = get(&app(), uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page
        .as_str()
        .unwrap()
        .contains("code-${wcaId}-public,manage_registrations`"));

    let uri = "/oauth/authorize?redirect_uri=http://localhost/auth/login&scope=everything&response_type=code&client_id=example-application-id";
    assert_eq!(get(&app(), uri, None).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn authorize_page_keeps_the_state_for_the_redirect() {
    let uri = "/oauth/authorize?redirect_uri=http://localhost/auth/login&scope=public&response_type=code&client_id=example-application-id&state=abc-123";
    let (status, page) = get(&app(), uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.as_str().unwrap().contains("abc-123"));
}

#[tokio::test]
async fn users_search_wraps_matching_users_in_a_result() {
    let app = app();
    let (status, found) = get(&app, "/api/v0/users/search/VAQR", None).await;
    assert_eq!(status, StatusCode::OK);
    let result = found["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["wca_id"], "2012VAQR01");
    assert_eq!(result[0]["id"], 9000001);
    assert!(result[0]["name"].is_string());

    let (_, found) = get(&app, "/api/v0/users/search/nobody-like-this", None).await;
    assert_eq!(found["result"], json!([]));
}

#[tokio::test]
async fn reset_by_id_leaves_other_competitions_alone() {
    let app = app();
    let generated = format!("/api/v0/competitions/{}/wcif", ID);
    let snapshot = "/api/v0/competitions/WLSMarzec2026/wcif";
    // Empties registrant 1's assignments, found the way the WCA does: by wcaUserId
    let empty_for = |wcif: &Value| {
        let first = wcif["persons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["registrantId"] == 1)
            .unwrap();
        json!({ "persons": [{ "wcaUserId": first["wcaUserId"], "assignments": [] }] })
    };
    let (_, wcif) = get(&app, &format!("{}/version/2", generated), Some(DELEGATE)).await;
    let empty_generated = empty_for(&wcif);
    let (_, wcif) = get(&app, &format!("{}/version/2", snapshot), None).await;
    let empty_snapshot = empty_for(&wcif);
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &generated,
            Some(DELEGATE),
            Some(empty_generated)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            snapshot,
            Some("token-2013ROGA02"),
            Some(empty_snapshot)
        )
        .await
        .0,
        StatusCode::OK
    );

    let (_, cleared) = call(
        &app,
        Method::POST,
        &format!("/dummy/reset?id={}", ID),
        None,
        None,
    )
    .await;
    assert_eq!(cleared["clearedCompetitions"], 1);

    let assignments = |wcif: &Value| {
        wcif["persons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["registrantId"] == 1)
            .unwrap()["assignments"]
            .clone()
    };
    let (_, wcif) = get(
        &app,
        "/api/v0/competitions/WLSMarzec2026/wcif/version/2",
        None,
    )
    .await;
    assert_eq!(assignments(&wcif), json!([]), "the snapshot patch survives");
    let (_, wcif) = get(
        &app,
        &format!("/api/v0/competitions/{}/wcif/version/2", ID),
        Some(DELEGATE),
    )
    .await;
    assert_ne!(assignments(&wcif), json!([]), "the generated patch is gone");
}

async fn submit(app: &Router, uri: &str, form: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(form.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into())
}

const SMALL: &str = "name=Small+Test+Open&start_date=2026-11-14&days=1&competitors=20&rooms=1\
    &timezone=Europe%2FBerlin&seed=7&event_333=2&event_pyram=1";

#[tokio::test]
async fn index_lists_generated_and_snapshot_competitions() {
    let (status, page) = get(&app(), "/", None).await;
    assert_eq!(status, StatusCode::OK);
    let page = page.as_str().unwrap();
    assert!(page.contains(ID));
    assert!(page.contains("WLSMarzec2026"));
    assert!(page.contains("name=\"event_333\""));
}

#[tokio::test]
async fn created_competition_is_served_and_managed() {
    let app = app();
    let (status, _) = submit(&app, "/dummy/competitions", SMALL).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, wcif) = get(
        &app,
        "/api/v0/competitions/SmallTestOpen2026/wcif/version/2",
        Some(DELEGATE),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(wcif["schedule"]["startDate"], "2026-11-14");
    assert_eq!(wcif["schedule"]["venues"][0]["timezone"], "Europe/Berlin");
    let events: Vec<&str> = wcif["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(events, ["333", "pyram"]);
    assert_eq!(wcif["persons"].as_array().unwrap().len(), 20);

    let (_, listed) = get(
        &app,
        "/api/v0/competitions?managed_by_me=true",
        Some(DELEGATE),
    )
    .await;
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [ID, "SmallTestOpen2026"]);

    let (_, discovery) = get(&app, "/dummy/generated", None).await;
    assert_eq!(
        discovery["id"], ID,
        "the competition from the settings stays the discovered one"
    );
}

#[tokio::test]
async fn creating_rejects_a_taken_id_and_bad_settings() {
    let app = app();
    submit(&app, "/dummy/competitions", SMALL).await;
    let (status, page) = submit(&app, "/dummy/competitions", SMALL).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("already exists"));

    let taken = "name=Dummy+Cubing+Open&start_date=2026-01-05&event_333=1";
    let (status, _) = submit(&app, "/dummy/competitions", taken).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "same id as the competition from the settings"
    );

    let (status, page) = submit(
        &app,
        "/dummy/competitions",
        "name=Other&start_date=2026-11-14&days=9&event_333=1",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("Days must be a number from 1 to 4"));
    assert!(
        page.contains("value=\"Other\""),
        "the form keeps what was sent"
    );

    let (status, page) = submit(
        &app,
        "/dummy/competitions",
        "name=Other&start_date=2026-11-14",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("Pick at least one event"));

    let crowded = "name=Other&start_date=2026-11-14&days=1&competitors=1000&rooms=1&event_333=4&event_444=4&event_555=4";
    let (status, page) = submit(&app, "/dummy/competitions", crowded).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("the schedule doesn&#x27;t fit"));
    assert!(!page.contains("GENERATED_"), "errors name the form fields");
}

#[tokio::test]
async fn created_competition_can_be_removed_but_not_the_configured_one() {
    let app = app();
    submit(&app, "/dummy/competitions", SMALL).await;
    let (status, _) = submit(&app, "/dummy/competitions/SmallTestOpen2026/delete", "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _) = get(&app, "/api/v0/competitions/SmallTestOpen2026", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = submit(&app, &format!("/dummy/competitions/{}/delete", ID), "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn competitions_say_how_results_are_entered() {
    let app = app();
    let (status, info) = get(&app, &format!("/api/v0/competitions/{}", ID), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["scoretaking_software"], "internal");

    let form =
        "name=Paper+Test+Open&start_date=2026-11-14&event_333=1&scoretaking_software=external";
    assert_eq!(
        submit(&app, "/dummy/competitions", form).await.0,
        StatusCode::SEE_OTHER
    );
    let (_, info) = get(&app, "/api/v0/competitions/PaperTestOpen2026", None).await;
    assert_eq!(info["scoretaking_software"], "external");

    let form = "name=Other&start_date=2026-11-14&event_333=1&scoretaking_software=paper";
    let (status, page) = submit(&app, "/dummy/competitions", form).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("Scoretaking software must be one of"));
}

#[tokio::test]
async fn wcif_persons_carry_their_registration_id() {
    let (_, wcif) = get(
        &app(),
        &format!("/api/v0/competitions/{}/wcif/public", ID),
        None,
    )
    .await;
    let ids: Vec<i64> = wcif["persons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|person| {
            person["registration"]["wcaRegistrationId"]
                .as_i64()
                .unwrap()
        })
        .collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert!(!ids.is_empty());
    assert_eq!(unique.len(), ids.len(), "registration ids are unique");
}
