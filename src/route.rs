use axum::{
    middleware,
    routing::{delete, get, patch, post, put},
    Router,
};

use crate::dashboard::{create_handler, index_handler, remove_handler};
use crate::handler::{
    check_wcif_handler, code_handler, get_competition_info_handler, get_competitions_handler,
    get_generated_handler, get_me_handler, get_person_handler, get_public_wcif_handler,
    get_records_handler, get_token_handler, get_user_by_wca_id_handler, get_wcif_handler,
    get_wcif_v2_handler, patch_wcif_handler, reset_handler, search_users_by_path_handler,
    search_users_handler,
};
use crate::live;
use crate::state::SharedState;
use crate::v1;

/// The WCA's `/api/v1` endpoints, rate limited like there.
fn v1_router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .route(
            "/api/v1/registrations/:id",
            patch(v1::registrations::update),
        )
        .route(
            "/api/v1/competitions/:id/scoretakers",
            get(v1::scoretakers::index).post(v1::scoretakers::create),
        )
        .route(
            "/api/v1/competitions/:id/scoretakers/:user_id",
            delete(v1::scoretakers::destroy),
        )
        .merge(live_router())
        .route_layer(middleware::from_fn_with_state(state, v1::rate_limit))
}

/// The WCA's live results API, under `/api/v1/competitions/:id/live`.
fn live_router() -> Router<SharedState> {
    let rounds = "/api/v1/competitions/:id/live/rounds";
    Router::new()
        .route(rounds, get(live::rounds_info))
        .route(
            &format!("{}/:round_id", rounds),
            get(live::round_results)
                .post(live::writes::add_or_update_result)
                .patch(live::writes::add_or_update_result),
        )
        .route(
            &format!("{}/:round_id/batch", rounds),
            post(live::writes::batch_add_or_update_results),
        )
        .route(&format!("{}/:round_id/open", rounds), put(live::open_round))
        .route(
            &format!("{}/:round_id/clear", rounds),
            put(live::clear_round),
        )
        .route(
            &format!("{}/:round_id/close", rounds),
            delete(live::close_round),
        )
        .route(
            &format!("{}/:round_id/bulk_quit", rounds),
            delete(live::bulk_quit_competitors),
        )
        .route(
            &format!("{}/:round_id/next_if_quit", rounds),
            get(live::next_if_quit),
        )
        .route(
            &format!("{}/:round_id/addable_competitors", rounds),
            get(live::addable_competitors),
        )
        .route(
            &format!("{}/:round_id/:registration_id", rounds),
            put(live::add_competitor).delete(live::quit_competitor),
        )
        .route(
            &format!("{}/:round_id/:registration_id/clear", rounds),
            put(live::clear_competitor),
        )
}

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .merge(v1_router(state.clone()))
        .route("/", get(index_handler))
        .route("/dummy/competitions", post(create_handler))
        .route("/dummy/competitions/:id/delete", post(remove_handler))
        .route("/oauth/authorize", get(code_handler))
        .route("/oauth/token", post(get_token_handler))
        .route("/api/v0/me", get(get_me_handler))
        .route("/api/v0/users/:id", get(get_user_by_wca_id_handler))
        .route(
            "/api/v0/users/search/:query",
            get(search_users_by_path_handler),
        )
        .route("/api/v0/persons", get(search_users_handler))
        .route("/api/v0/persons/:id", get(get_person_handler))
        .route("/api/v0/competitions", get(get_competitions_handler))
        .route(
            "/api/v0/competitions/:id",
            get(get_competition_info_handler),
        )
        .route("/api/v0/competitions/wcif/check", put(check_wcif_handler))
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
        .route(
            "/dummy/competitions/:id/live/drop_next_write",
            post(live::writes::drop_next_write),
        )
        .with_state(state)
}
