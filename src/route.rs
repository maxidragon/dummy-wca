use axum::{
    routing::{get, post},
    Router,
};

use crate::handler::{code_handler, get_competitions_handler, get_me_handler, get_public_wcif_handler, get_competition_info_handler, get_records_handler, get_token_handler, get_user_by_wca_id_handler, get_wcif_handler, search_users_handler};

pub fn create_router() -> Router {
    Router::new()
        .route("/oauth/authorize", get(code_handler))
        .route("/oauth/token", post(get_token_handler))
        .route("/api/v0/me", get(get_me_handler))
        .route("/api/v0/users/:id", get(get_user_by_wca_id_handler))
        .route("/api/v0/persons", get(search_users_handler))
        .route("/api/v0/competitions", get(get_competitions_handler))
        .route("/api/v0/competitions/:id", get(get_competition_info_handler))
        .route("/api/v0/competitions/:id/wcif", get(get_wcif_handler))
        .route("/api/v0/competitions/:id/wcif/public", get(get_public_wcif_handler))
        .route("/api/v0/records", get(get_records_handler))
}
