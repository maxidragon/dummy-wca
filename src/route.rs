use axum::{
    routing::{get, post},
    Router,
};

use crate::dashboard::{create_handler, index_handler, remove_handler};
use crate::handler::{
    code_handler, get_competition_info_handler, get_competitions_handler, get_generated_handler,
    get_me_handler, get_person_handler, get_public_wcif_handler, get_records_handler, get_token_handler,
    get_user_by_wca_id_handler, get_wcif_handler, get_wcif_v2_handler, patch_wcif_handler, reset_handler,
    search_users_handler,
};
use crate::state::SharedState;

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/dummy/competitions", post(create_handler))
        .route("/dummy/competitions/:id/delete", post(remove_handler))
        .route("/oauth/authorize", get(code_handler))
        .route("/oauth/token", post(get_token_handler))
        .route("/api/v0/me", get(get_me_handler))
        .route("/api/v0/users/:id", get(get_user_by_wca_id_handler))
        .route("/api/v0/persons", get(search_users_handler))
        .route("/api/v0/persons/:id", get(get_person_handler))
        .route("/api/v0/competitions", get(get_competitions_handler))
        .route(
            "/api/v0/competitions/:id",
            get(get_competition_info_handler),
        )
        .route(
            "/api/v0/competitions/:id/wcif",
            get(get_wcif_handler).patch(patch_wcif_handler),
        )
        .route(
            "/api/v0/competitions/:id/wcif/version/2",
            get(get_wcif_v2_handler),
        )
        .route(
            "/api/v0/competitions/:id/wcif/public",
            get(get_public_wcif_handler),
        )
        .route("/api/v0/records", get(get_records_handler))
        .route("/dummy/generated", get(get_generated_handler))
        .route("/dummy/reset", post(reset_handler))
        .with_state(state)
}
