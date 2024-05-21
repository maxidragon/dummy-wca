use axum::{
    extract::{Path, Query}, http::StatusCode, response::{IntoResponse, Redirect}, Json
};

use axum_extra::{headers::{self, authorization::Bearer}, TypedHeader};
use serde_json::json;

use crate::schema::{CompetitionsListOptions, OauthRequestOptions, OauthTokenRequestOptions};

use crate::utils::read_json_file;

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
        let redirect = Redirect::to(&format!("{}?code={}", params.redirect_uri, "example-code"));
        return Ok(redirect.into_response());
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
    return Ok(Json(json!({"access_token": "example-access-token"})));
}

pub async fn get_me_handler(
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    if token_str != "example-access-token" {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error","message": "Unauthorized"})),
        ))
    }
    let file_path = "data/me.json";
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

pub async fn get_competitions_handler(
    Query(params): Query<CompetitionsListOptions>
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if params.managed_by_me == false {
        return Ok(Json(json!([])));
    }
    let file_path = "data/manageable-competitions.json";
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
    let file_path = "data/wcif.json";
    match read_json_file(file_path) {
        Ok(data) => {
            if id == "SLSFinal2024" {
                Ok(Json(data))
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({"status": "error","message": "ID not found"})),
                ))
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

pub async fn get_wcif_handler(
    Path(id): Path<String>,
    bearer_token: TypedHeader<headers::Authorization<Bearer>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token_str = bearer_token.token();
    if token_str != "example-access-token" {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error","message": "Unauthorized"})),
        ))
    }
    let file_path = "data/wcif.json";
    match read_json_file(file_path) {
        Ok(data) => {
            if id == "SLSFinal2024" {
                Ok(Json(data))
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({"status": "error","message": "ID not found"})),
                ))
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