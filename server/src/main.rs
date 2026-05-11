mod api;
mod db;

use std::path::{Component, PathBuf};
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use minisql::ConnectionPool;
use tokio::fs;

struct ServerState {
    db: ConnectionPool,
    jwt_secret: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO change to a configurable value in the future.
    let db_url = "sqlite:///tmp/server.db";
    let jwt_secret = std::env::var("FRIDAY_JWT_SECRET")
        .unwrap_or_else(|_| "change-this-development-secret".to_string());

    let db = ConnectionPool::new(db_url).unwrap();
    db.initialize_database(db::get_migrations()).await.unwrap();

    let state = Arc::new(ServerState { db, jwt_secret });

    let app = app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn app(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .route("/api/logout", post(api::auth::logout))
        .route("/", get(frontend_asset))
        .fallback(get(frontend_asset))
        .with_state(state)
}

async fn frontend_asset(uri: Uri) -> Response {
    let Some(path) = frontend_path(uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, content_type(&path).to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn frontend_path(request_path: &str) -> Option<PathBuf> {
    let mut path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/frontend"));
    let request_path = request_path.trim_start_matches('/');

    if request_path.is_empty() || request_path == "login" || request_path == "register" {
        path.push("index.html");
        return Some(path);
    }

    for component in PathBuf::from(request_path).components() {
        match component {
            Component::Normal(part) => path.push(part),
            _ => return None,
        }
    }

    Some(path)
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("map") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}
