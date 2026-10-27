mod dashboard;
mod generator;
mod handler;
mod route;
mod schema;
mod state;
mod utils;

#[cfg(test)]
mod api_tests;

use std::sync::Arc;

use axum::http::{header::{AUTHORIZATION, CONTENT_TYPE}, Method};

use tokio::net::TcpListener;

use generator::GeneratorConfig;
use route::create_router;
use state::AppState;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());

    let config = GeneratorConfig::from_env().unwrap_or_else(|err| {
        eprintln!("Invalid generated competition settings: {}", err);
        std::process::exit(1);
    });
    let state = Arc::new(AppState::new(config));
    if let Err(err) = generator::check(&state.config, state.competition().start_date) {
        eprintln!("Invalid generated competition settings: {}", err);
        std::process::exit(1);
    }

    let app = create_router(state.clone()).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_headers([AUTHORIZATION, CONTENT_TYPE])
            .allow_methods([Method::GET, Method::POST, Method::PATCH]),
    );

    let competition = state.competition();
    println!(
        "Generated competition {} ({}, {} day(s), {} persons)",
        competition.id,
        competition.start_date,
        competition.number_of_days,
        competition.persons.len()
    );
    println!("Server started successfully on port {}", port);
    println!("Competitions and a form to generate new ones: http://localhost:{}/", port);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
