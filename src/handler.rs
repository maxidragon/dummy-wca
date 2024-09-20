use axum::{
    extract::{Path, Query}, http::{header, StatusCode}, response::{Html, IntoResponse, Response}, Json
};
use askama::Template;

use axum_extra::{headers::{self, authorization::Bearer}, TypedHeader};
use serde_json::json;

use crate::schema::{OauthRequestOptions, OauthTokenRequestOptions, SearchUsersOptions};

use crate::utils::read_json_file;

#[derive(Template)]
#[template(path = "authorize.html")]
struct AuthorizeTemplate {
    redirect_uri: String,
}

const ADMIN_TEAMS: [&str; 3] = ["wst", "wrt", "wcat"];

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
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    let wca_id = token_str.split("-").collect::<Vec<&str>>()[1];
    if token_str == "" {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error","message": "Unauthorized"})),
        ))
    }
    let file_path = &format!("data/users/{}/me.json", wca_id);
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

pub async fn get_user_by_wca_id_handler(
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let file_path = &format!("data/users/{}/me.json", id);
    match read_json_file(file_path) {
        Ok(mut data) => {
            data["user"] = data["me"].clone();
            data["me"] = json!({});
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

pub async fn search_users_handler(
    Query(params): Query<SearchUsersOptions>
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let file_path = "data/persons/persons.json";
    match read_json_file(file_path) {
        Ok(mut data) => {
            data = json!(data["persons"].as_array().unwrap().iter().filter(|person| {
                let person = person.as_object().unwrap()["person"].as_object().unwrap();
                let name = person["name"].as_str().unwrap();
                let wca_id = person["wca_id"].as_str().unwrap();
                name.to_lowercase().contains(&params.q) || wca_id.to_lowercase().contains(&params.q)
            }).map(|person| {
                return person.as_object().unwrap();
            }).collect::<Vec<_>>());
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

pub async fn get_competitions_handler(
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    let wca_id = token_str.split("-").collect::<Vec<&str>>()[1];
    let file_path = &format!("data/users/{}/manageable-competitions.json", wca_id);
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

pub async fn get_public_wcif_handler(
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let file_path = &format!("data/competitions/{}.json", id);
    match read_json_file(file_path) {
        Ok(data) => {
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

pub async fn get_wcif_handler(
    Path(id): Path<String>,
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    let wca_id = token_str.split("-").collect::<Vec<&str>>()[1];
    if token_str == "" {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error","message": "Unauthorized"})),
        ))
    }
    let manageable_competitions_file_path = &format!("data/users/{}/manageable-competitions.json", wca_id);
    let user_info_path = &format!("data/users/{}/me.json", wca_id);
    let mut has_access = false;
    match read_json_file(manageable_competitions_file_path) {
        Ok(manageable_competitions) => {
            for competition in manageable_competitions.as_array().unwrap() {
                if competition["id"].as_str().unwrap() == id {
                    has_access = true;
                    break;
                }
            }
        }
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error","message": "Something went wrong"})),
            ));
        }
    }
    if !has_access {
        match read_json_file(user_info_path) {
            Ok(user_info) => {
                let user = user_info["me"].as_object().unwrap();
                if user["teams"].as_array().unwrap().iter().any(|team| ADMIN_TEAMS.contains(&team.as_str().unwrap())) {
                    has_access = true;
                }
            }
            Err(_) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({"status": "error","message": "Something went wrong"})),
                ));
            }
        }
    }
    if !has_access {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error","message": "Forbidden resource"})),
        ))
    }
    let file_path = &format!("data/competitions/{}.json", id);
    match read_json_file(file_path) {
        Ok(data) => {
            Ok(Json(data))
        }
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": "error","message": "ID not found"})),
            ));
        }
    }
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