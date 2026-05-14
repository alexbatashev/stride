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
use friday_agent::{AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry};
use handlebars::Handlebars;
use llm::{Anthropic, Ollama, OpenAI};
use minisql::ConnectionPool;
use tower_http::services::ServeDir;

use crate::{pages::get_templates, runner::AgentPool};

struct ServerState {
    #[allow(dead_code)]
    pub(crate) config: config::Config,
    pub(crate) db: ConnectionPool,
    pub(crate) jwt_secret: String,
    pub(crate) runner: Arc<dyn AgentPool>,
    pub(crate) templates: Handlebars<'static>,
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
    let runner = Arc::new(runner::inproc::InProcessAgentPool::new(
        db.clone(),
        Arc::new(AgentConfig {
            model_registry: create_model_registry(&config),
            max_iterations: 90,
        }),
    ));

    let state = Arc::new(ServerState {
        config,
        db,
        jwt_secret,
        runner,
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
        .route(
            "/api/threads",
            get(api::threads::list_threads).post(api::threads::create_thread),
        )
        .route(
            "/api/threads/{id}/messages",
            get(api::threads::list_messages).post(api::threads::send_message),
        )
        .route("/api/threads/{id}/events", get(api::threads::events))
        .route("/auth/login", get(pages::auth::login))
        .route("/auth/register", get(pages::auth::register))
        .route("/threads", get(pages::agent::new_thread))
        .route("/threads/{id}", get(pages::agent::thread))
        .route("/", get(root))
        .nest_service("/static", ServeDir::new(static_dir))
        .with_state(state)
}

fn create_model_registry(config: &config::Config) -> ModelRegistry {
    let mut registry = ModelRegistry::new();

    for (name, model) in &config.models {
        let Some(provider) = config.providers.get(&model.provider) else {
            continue;
        };
        let api = match provider.kind {
            config::Kind::OpenAI => OpenAI::new(&provider.url),
            config::Kind::Anthropic => Anthropic::new(&provider.url),
            config::Kind::Ollama => Ollama::new(&provider.url),
        };
        registry.add_model(
            name,
            ModelRegEntry {
                api,
                token: provider
                    .read_token(&model.provider)
                    .unwrap_or("-".to_string()),
                model_name: model.slug.clone(),
                thinking: model.thinking.unwrap_or(true),
            },
        );
    }

    if !config.models.contains_key(DEFAULT_MODEL) {
        if let Some((_, model)) = config.models.iter().next() {
            if let Some(provider) = config.providers.get(&model.provider) {
                let api = match provider.kind {
                    config::Kind::OpenAI => OpenAI::new(&provider.url),
                    config::Kind::Anthropic => Anthropic::new(&provider.url),
                    config::Kind::Ollama => Ollama::new(&provider.url),
                };
                registry.add_model(
                    DEFAULT_MODEL,
                    ModelRegEntry {
                        api,
                        token: provider
                            .read_token(&model.provider)
                            .unwrap_or("-".to_string()),
                        model_name: model.slug.clone(),
                        thinking: model.thinking.unwrap_or(true),
                    },
                );
            }
        }
    }

    registry
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
