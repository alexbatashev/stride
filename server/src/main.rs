mod api;
mod db;

use std::sync::Arc;

use axum::{Router, routing::post};
use minisql::ConnectionPool;

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
        .with_state(state)
}
