mod api;
mod db;
mod pages;
pub mod runner;

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use handlebars::Handlebars;
use minisql::ConnectionPool;
use tower_http::services::ServeDir;

use crate::pages::get_templates;

struct ServerState {
    db: ConnectionPool,
    jwt_secret: String,
    templates: Handlebars<'static>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO change to a configurable value in the future.
    let db_url = "sqlite:///tmp/server.db";
    let jwt_secret = std::env::var("FRIDAY_JWT_SECRET")
        .unwrap_or_else(|_| "change-this-development-secret".to_string());

    let db = ConnectionPool::new(db_url).unwrap();
    db.initialize_database(db::get_migrations()).await.unwrap();

    let templates = get_templates()?;

    let state = Arc::new(ServerState {
        db,
        jwt_secret,
        templates,
    });

    let app = app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn app(state: Arc<ServerState>) -> Router {
    let static_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend/dist");

    Router::new()
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .route("/api/logout", post(api::auth::logout))
        .route("/auth/login", get(pages::auth::login))
        .route("/auth/register", get(pages::auth::register))
        .route("/threads", get(pages::agent::new_thread))
        .route("/", get(root))
        .nest_service("/static", ServeDir::new(static_dir))
        .with_state(state)
}

async fn root(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let path = if is_authenticated(&state, &headers) {
        "/threads"
    } else {
        "/auth/login"
    };
    Redirect::to(path).into_response()
}

fn is_authenticated(state: &ServerState, headers: &HeaderMap) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|part| part.trim().strip_prefix("token="))
        })
        .map(|token| api::auth::verify_token(&state.jwt_secret, token).is_ok())
        .unwrap_or(false)
}
