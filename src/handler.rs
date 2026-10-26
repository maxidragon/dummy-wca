use std::collections::HashSet;

use axum::{
    extract::{Path, Query, State}, http::{header, StatusCode}, response::{Html, IntoResponse, Response}, Json
};
use askama::Template;

use axum_extra::{headers::{self, authorization::Bearer}, TypedHeader};
use chrono::NaiveDate;
use serde_json::{json, Value};

use crate::generator::{names::{self, Account, ACCOUNTS}, wcif::{self, Version}, Competition};
use crate::schema::{
    ManagedCompetitionsOptions, OauthRequestOptions, OauthTokenRequestOptions, ResetOptions, SearchUsersOptions,
};
use crate::state::SharedState;

use crate::utils::read_json_file;

type ApiResult = Result<Response, (StatusCode, Json<Value>)>;

#[derive(Template)]
#[template(path = "authorize.html")]
struct AuthorizeTemplate {
    redirect_uri: String,
    accounts: &'static [Account],
}

const ADMIN_TEAMS: [&str; 3] = ["wst", "wrt", "wcat"];

fn error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"status": "error","message": message})))
}

/// Tokens look like `token-<WCA ID>`.
fn wca_id_from_token(token: &str) -> Option<&str> {
    token.split_once('-').map(|(_, wca_id)| wca_id).filter(|wca_id| !wca_id.is_empty())
}

impl IntoResponse for AuthorizeTemplate {
    fn into_response(self) -> Response {
        match self.render() {
            Ok(html) => {
                let mut response = Html(html).into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/html"),
                );
                response
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

pub async fn code_handler(
    params: Query<OauthRequestOptions>
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let client_id = params.client_id.clone();
    if client_id != "example-application-id" {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"status": "error","message": "Invalid client_id"})),
        ));
    } else {
        let template = AuthorizeTemplate {
            redirect_uri: params.redirect_uri.clone(),
            accounts: &ACCOUNTS,
        };
        return Ok(template.into_response());
    }
}

pub async fn get_token_handler(
    Json(body): Json<OauthTokenRequestOptions>
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if body.client_id != "example-application-id" || body.client_secret != "example-secret" || body.grant_type != "authorization_code" {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"status": "error","message": "Invalid data"})),
        ));
    }
    let wca_id = body.code.split("-").collect::<Vec<&str>>()[1];
    return Ok(Json(json!({"access_token": format!("token-{}", wca_id)})));
}

pub async fn get_me_handler(
    State(state): State<SharedState>,
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    let Some(wca_id) = wca_id_from_token(token_str) else {
        return Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };
    let file_path = &format!("data/users/{}/me.json", wca_id);
    match read_json_file(file_path) {
        Ok(data) => {
            return Ok(Json(data));
        }
        Err(_) => match wcif::user_by_wca_id(&state.competition(), wca_id) {
            Some(user) => Ok(Json(json!({ "me": user }))),
            None => Err(error(StatusCode::NOT_FOUND, "Something went wrong")),
        },
    }
}

pub async fn get_user_by_wca_id_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let file_path = &format!("data/users/{}/me.json", id);
    match read_json_file(file_path) {
        Ok(mut data) => {
            data["user"] = data["me"].clone();
            data["me"] = json!({});
            Ok(Json(data))
        }
        Err(_) => match wcif::user_by_wca_id(&state.competition(), &id) {
            Some(user) => Ok(Json(json!({ "user": user }))),
            None => Err(error(StatusCode::NOT_FOUND, "ID not found")),
        },
    }
}

fn snapshot_persons() -> Vec<Value> {
    read_json_file("data/persons/persons.json")
        .ok()
        .and_then(|data| data["persons"].as_array().cloned())
        .unwrap_or_default()
}

pub async fn search_users_handler(
    State(state): State<SharedState>,
    Query(params): Query<SearchUsersOptions>
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let query = params.q.to_lowercase();
    let matches = |entry: &Value| {
        let person = &entry["person"];
        let name = person["name"].as_str().unwrap_or_default();
        let wca_id = person["wca_id"].as_str().unwrap_or_default();
        name.to_lowercase().contains(&query) || wca_id.to_lowercase().contains(&query)
    };
    let found: Vec<Value> = snapshot_persons()
        .into_iter()
        .chain(wcif::person_entries(&state.competition()))
        .filter(|entry| matches(entry))
        .collect();
    Ok(Json(json!(found)))
}

pub async fn get_person_handler(
    State(state): State<SharedState>,
    Path(wca_id): Path<String>,
) -> ApiResult {
    snapshot_persons()
        .into_iter()
        .chain(wcif::person_entries(&state.competition()))
        .find(|entry| {
            entry["person"]["wca_id"]
                .as_str()
                .is_some_and(|id| id.eq_ignore_ascii_case(&wca_id))
        })
        .map(|entry| Json(entry).into_response())
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Person not found"))
}

/// Like the WCA, `start` keeps competitions that end on or after it and `end`
/// keeps those that start on or before it. Both accept a date or an ISO timestamp.
fn within_dates(competition: &Competition, params: &ManagedCompetitionsOptions) -> bool {
    let parse = |value: &Option<String>| {
        value
            .as_deref()
            .and_then(|value| value.get(..10))
            .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
    };
    parse(&params.start).is_none_or(|start| competition.end_date() >= start)
        && parse(&params.end).is_none_or(|end| competition.start_date <= end)
}

pub async fn get_competitions_handler(
    State(state): State<SharedState>,
    Query(params): Query<ManagedCompetitionsOptions>,
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    let Some(wca_id) = wca_id_from_token(token_str) else {
        return Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };
    let file_path = &format!("data/users/{}/manageable-competitions.json", wca_id);
    match read_json_file(file_path) {
        // Snapshot accounts keep their full, unfiltered lists: FKMTime still imports past snapshots.
        Ok(data) => {
            return Ok(Json(data));
        }
        Err(_) => {
            let competition = state.competition();
            if wcif::user_by_wca_id(&competition, wca_id).is_none() {
                return Err(error(StatusCode::NOT_FOUND, "Something went wrong"));
            }
            let managed = wcif::manager_role(&competition, wca_id).is_some() && within_dates(&competition, &params);
            let listed: Vec<Value> = if managed { vec![wcif::listing(&competition)] } else { vec![] };
            Ok(Json(json!(listed)))
        }
    }
}

pub async fn get_public_wcif_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(competition) = state.generated(&id) {
        let mut data = wcif::wcif(&competition, Version::V1);
        state.apply_patches(&id, &mut data);
        return Ok(Json(wcif::public_view(data)));
    }
    let file_path = &format!("data/competitions/{}/wcif.json", id);
    match read_json_file(file_path) {
        Ok(mut data) => {
            state.apply_patches(&id, &mut data);
            Ok(Json(data))
        }
        Err(err) => {
            println!("{}", err);
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error","message": "ID not found"})),
            ))
        }
    }
}

pub async fn get_competition_info_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(competition) = state.generated(&id) {
        return Ok(Json(wcif::competition_info(&competition)));
    }
    let file_path = &format!("data/competitions/{}/competition.json", id);
    match read_json_file(file_path) {
        Ok(data) => {
            Ok(Json(data))
        }
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error","message": "ID not found"})),
            ))
        }
    }
}

/// Whether the token's user may read the private WCIF of the generated
/// competition: its delegates and organizers, and WCA admin teams.
fn can_manage_generated(competition: &Competition, wca_id: &str) -> Result<bool, (StatusCode, Json<Value>)> {
    if wcif::manager_role(competition, wca_id).is_some() {
        return Ok(true);
    }
    if let Some(account) = names::account(wca_id) {
        return Ok(account.team.is_some_and(|team| ADMIN_TEAMS.contains(&team)));
    }
    if let Ok(user_info) = read_json_file(&format!("data/users/{}/me.json", wca_id)) {
        return Ok(has_admin_team(&user_info));
    }
    if wcif::user_by_wca_id(competition, wca_id).is_some() {
        return Ok(false);
    }
    Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"))
}

/// Teams in `me.json` are objects with a `friendly_id` (`wst`, `wrt`, ...).
fn has_admin_team(user_info: &Value) -> bool {
    user_info["me"]["teams"].as_array().is_some_and(|teams| {
        teams.iter().any(|team| {
            team["friendly_id"]
                .as_str()
                .or(team.as_str())
                .is_some_and(|team| ADMIN_TEAMS.contains(&team))
        })
    })
}

fn generated_wcif(
    state: &SharedState,
    competition: &Competition,
    bearer_token: Option<TypedHeader<headers::Authorization<Bearer>>>,
    version: Version,
    allow_public: bool,
) -> ApiResult {
    let mut data = wcif::wcif(competition, version);
    state.apply_patches(&competition.id, &mut data);
    let Some(bearer_token) = bearer_token else {
        if allow_public {
            return Ok(Json(wcif::public_view(data)).into_response());
        }
        return Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };
    let Some(wca_id) = wca_id_from_token(bearer_token.token()) else {
        return Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };
    if !can_manage_generated(competition, wca_id)? {
        return Err(error(StatusCode::FORBIDDEN, "Forbidden resource"));
    }
    Ok(Json(data).into_response())
}

pub async fn get_wcif_v2_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    bearer_token: Option<TypedHeader<headers::Authorization<Bearer>>>,
) -> ApiResult {
    if let Some(competition) = state.generated(&id) {
        // Without a token this is the public WCIF, which is how FKMTime fetches it.
        return generated_wcif(&state, &competition, bearer_token, Version::V2, true);
    }
    let file_path = format!("data/competitions/{}/wcif_v2.json", id);
    match read_json_file(&file_path) {
        Ok(mut data) => {
            state.apply_patches(&id, &mut data);
            Ok(Json(data).into_response())
        }
        Err(_) => Err(error(StatusCode::NOT_FOUND, "ID not found")),
    }
}

pub async fn get_wcif_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    bearer_token: Option<TypedHeader<headers::Authorization<Bearer>>>,
) -> ApiResult {
    if let Some(competition) = state.generated(&id) {
        return generated_wcif(&state, &competition, bearer_token, Version::V1, false);
    }
    let token_str = match &bearer_token {
        Some(token) => token.token(),
        None => "",
    };
    let Some(wca_id) = wca_id_from_token(token_str) else {
        return Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };
    if !can_manage_snapshot(&id, wca_id)? {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error","message": "Forbidden resource"})),
        ))
    }
    let file_path = &format!("data/competitions/{}/wcif.json", id);
    match read_json_file(file_path) {
        Ok(mut data) => {
            state.apply_patches(&id, &mut data);
            Ok(Json(data).into_response())
        }
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error","message": "ID not found"})),
            ));
        }
    }
}

/// Snapshot competitions: listed in the user's manageable competitions, or
/// the user is in a WCA admin team.
fn can_manage_snapshot(id: &str, wca_id: &str) -> Result<bool, (StatusCode, Json<Value>)> {
    let manageable_competitions_file_path = &format!("data/users/{}/manageable-competitions.json", wca_id);
    let user_info_path = &format!("data/users/{}/me.json", wca_id);
    let manageable_competitions = read_json_file(manageable_competitions_file_path)
        .map_err(|_| error(StatusCode::NOT_FOUND, "Something went wrong"))?;
    let listed = manageable_competitions
        .as_array()
        .is_some_and(|competitions| competitions.iter().any(|competition| competition["id"] == id));
    if listed {
        return Ok(true);
    }
    let user_info = read_json_file(user_info_path).map_err(|_| error(StatusCode::NOT_FOUND, "Something went wrong"))?;
    Ok(has_admin_team(&user_info))
}

fn activity_ids(activities: &Value, ids: &mut HashSet<i64>) {
    for activity in activities.as_array().into_iter().flatten() {
        if let Some(id) = activity["id"].as_i64() {
            ids.insert(id);
        }
        activity_ids(&activity["childActivities"], ids);
    }
}

/// Checks a PATCH body against the competition's WCIF and returns the persons
/// to save. Only `persons` can be patched; `id` and `formatVersion` may be sent.
fn validate_patch(id: &str, body: &Value, base: &Value) -> Result<Vec<serde_json::Map<String, Value>>, String> {
    let body = body.as_object().ok_or("The body must be a WCIF object")?;
    if let Some(unsupported) = body.keys().find(|key| !["id", "formatVersion", "persons"].contains(&key.as_str())) {
        return Err(format!("dummy-wca can only patch persons, not {:?}", unsupported));
    }
    if body.get("id").is_some_and(|body_id| body_id != id) {
        return Err(format!("The WCIF id does not match {}", id));
    }
    let persons = match body.get("persons") {
        None => return Ok(Vec::new()),
        Some(persons) => persons.as_array().ok_or("persons must be an array")?,
    };

    let registrant_ids: HashSet<i64> = base["persons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|person| person["registrantId"].as_i64())
        .collect();
    let mut known_activities = HashSet::new();
    for venue in base["schedule"]["venues"].as_array().into_iter().flatten() {
        for room in venue["rooms"].as_array().into_iter().flatten() {
            activity_ids(&room["activities"], &mut known_activities);
        }
    }

    persons
        .iter()
        .map(|person| {
            let person = person.as_object().ok_or("Every person must be an object")?;
            let registrant_id = person
                .get("registrantId")
                .and_then(Value::as_i64)
                .ok_or("Every person needs a numeric registrantId")?;
            if !registrant_ids.contains(&registrant_id) {
                return Err(format!("No person has registrantId {}", registrant_id));
            }
            if let Some(assignments) = person.get("assignments") {
                for assignment in assignments.as_array().ok_or("assignments must be an array")? {
                    let activity_id = assignment["activityId"].as_i64().ok_or("Every assignment needs an activityId")?;
                    if !known_activities.contains(&activity_id) {
                        return Err(format!("No activity has id {}", activity_id));
                    }
                    if !assignment["assignmentCode"].is_string() {
                        return Err("Every assignment needs an assignmentCode".to_string());
                    }
                    if !(assignment["stationNumber"].is_null() || assignment["stationNumber"].is_i64()) {
                        return Err("stationNumber must be a number or null".to_string());
                    }
                }
            }
            Ok(person.clone())
        })
        .collect()
}

pub async fn patch_wcif_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    bearer_token: Option<TypedHeader<headers::Authorization<Bearer>>>,
    Json(body): Json<Value>,
) -> ApiResult {
    let token_str = bearer_token.as_ref().map(|token| token.token()).unwrap_or_default();
    let Some(wca_id) = wca_id_from_token(token_str) else {
        return Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };

    let base = match state.generated(&id) {
        Some(competition) => {
            if !can_manage_generated(&competition, wca_id)? {
                return Err(error(StatusCode::FORBIDDEN, "Forbidden resource"));
            }
            wcif::wcif(&competition, Version::V2)
        }
        None => {
            let base = read_json_file(&format!("data/competitions/{}/wcif_v2.json", id))
                .or_else(|_| read_json_file(&format!("data/competitions/{}/wcif.json", id)))
                .map_err(|_| error(StatusCode::NOT_FOUND, "ID not found"))?;
            if !can_manage_snapshot(&id, wca_id)? {
                return Err(error(StatusCode::FORBIDDEN, "Forbidden resource"));
            }
            base
        }
    };

    let persons = validate_patch(&id, &body, &base).map_err(|message| error(StatusCode::BAD_REQUEST, &message))?;
    state.save_patch(&id, persons);
    Ok(Json(json!({"status": "Successfully saved WCIF"})).into_response())
}

/// Test helper: what the generated competition is, and which accounts can
/// sign in to it. FKMTime's seed reads its id and dates from here.
pub async fn get_generated_handler(State(state): State<SharedState>) -> ApiResult {
    let competition = state.competition();
    let accounts: serde_json::Map<String, Value> = ACCOUNTS
        .iter()
        .map(|account| {
            (
                account.key.to_string(),
                json!({
                    "name": account.name,
                    "wcaId": account.wca_id,
                    "wcaUserId": account.user_id,
                    "role": wcif::manager_role(&competition, account.wca_id).or(account.team),
                    "code": format!("code-{}", account.wca_id),
                    "token": format!("token-{}", account.wca_id),
                    "description": account.description,
                }),
            )
        })
        .collect();
    Ok(Json(json!({
        "id": competition.id,
        "name": competition.name,
        "startDate": competition.start_date.format("%Y-%m-%d").to_string(),
        "endDate": competition.end_date().format("%Y-%m-%d").to_string(),
        "numberOfDays": competition.number_of_days,
        "timezone": competition.timezone.name(),
        "countryIso2": competition.host.country_iso2,
        // Pinned by GENERATED_START_DATE, or else today in the venue timezone
        // when the server started; it only moves on a restart.
        "startDatePinned": state.config.start_date.is_some(),
        "serverStartedAt": state.started_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "seed": state.config.seed,
        "competitors": competition.persons.len(),
        "acceptedCompetitors": competition.accepted().count(),
        "rooms": competition.venue.rooms.len(),
        "events": competition.events.iter().map(|event| json!({
            "id": event.info.id,
            "rounds": event.rounds.iter().map(|round| json!({
                "id": round.id,
                "expectedCompetitors": round.competitors,
                "groups": round.groups,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "accounts": accounts,
        "patchedPersons": state.patched_persons(&competition.id),
    }))
    .into_response())
}

pub async fn reset_handler(State(state): State<SharedState>, Query(params): Query<ResetOptions>) -> ApiResult {
    let cleared = state.reset(params.id.as_deref());
    Ok(Json(json!({ "status": "ok", "clearedCompetitions": cleared })).into_response())
}

pub async fn get_records_handler() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let file_path = "data/records.json";
    match read_json_file(file_path) {
        Ok(data) => {
            return Ok(Json(data));
        }
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error","message": "Something went wrong"})),
            ));
        }
    }
}
