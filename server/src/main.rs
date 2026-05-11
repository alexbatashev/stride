mod api;
mod db;

use std::sync::Arc;

use axum::{Router, routing::post};
use minisql::ConnectionPool;

struct ServerState {
    db: ConnectionPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO change to a configurable value in the future.
    let db_url = "sqlite:///tmp/server.db";

    let db = ConnectionPool::new(db_url).unwrap();

    let state = Arc::new(ServerState { db });

    let app = Router::new()
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .route("/api/logout", post(api::auth::logout))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
