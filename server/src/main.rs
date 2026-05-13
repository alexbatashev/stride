mod api;
mod config;
mod db;
mod pages;
pub mod runner;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use clap::Parser;
use handlebars::Handlebars;
use minisql::ConnectionPool;
use tower_http::services::ServeDir;

use crate::pages::get_templates;

struct ServerState {
    #[allow(dead_code)]
    pub(crate) config: config::Config,
    db: ConnectionPool,
    jwt_secret: String,
    templates: Handlebars<'static>,
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(short = 'c')]
    config_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = config::Config::load(&args.config_path)?;
    let jwt_secret = std::env::var("FRIDAY_JWT_SECRET")
        .unwrap_or_else(|_| "change-this-development-secret".to_string());

    let db = ConnectionPool::new(&config.db_url()).unwrap();
    db.initialize_database(db::get_migrations()).await.unwrap();

    let templates = get_templates()?;
    let listen_addr = config.listen_addr().to_string();

    let state = Arc::new(ServerState {
        config,
        db,
        jwt_secret,
        templates,
    });

    let app = app(state);

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_read_from_c_option() {
        let args = Args::try_parse_from(["server", "-c", "server.toml"]).unwrap();

        assert_eq!(args.config_path, PathBuf::from("server.toml"));
    }

    #[test]
    fn config_path_is_required() {
        let err = Args::try_parse_from(["server"]).unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }
}
