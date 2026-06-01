mod api;
mod components2;
mod config;
mod db;
mod pages;
pub mod runner;
mod tools;
mod vfs;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
    routing::{get, patch, post},
};
use clap::Parser;
use friday_agent::{
    AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry,
    mcp::{self, McpTool},
};
use handlebars::Handlebars;
use llm::{API, Anthropic, Ollama, OpenAI};
use minisql::ConnectionPool;
use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{pages::get_templates, runner::AgentPool};

const DEFAULT_STATIC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend/dist");

struct ServerState {
    #[allow(dead_code)]
    pub(crate) config: config::Config,
    pub(crate) db: ConnectionPool,
    pub(crate) jwt_secret: String,
    pub(crate) runner: Arc<dyn AgentPool>,
    pub(crate) model_config: Arc<AgentConfig>,
    pub(crate) templates: Handlebars<'static>,
    pub(crate) vfs: Option<Arc<vfs::Vfs>>,
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(short = 'c')]
    config_path: PathBuf,
    #[arg(long)]
    static_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let args = Args::parse();
    let config = config::Config::load(&args.config_path)?;
    let jwt_secret = std::env::var("FRIDAY_JWT_SECRET")
        .unwrap_or_else(|_| "change-this-development-secret".to_string());

    let db = ConnectionPool::new(&config.db_url()).unwrap();
    db.initialize_database(db::get_migrations()).await.unwrap();

    let templates = get_templates()?;
    let listen_addr = config.listen_addr().to_string();
    let tools = config.tools.clone().unwrap_or_default();
    if let Some(python) = tools.python.as_ref()
        && python.enabled.unwrap_or(true)
    {
        let python_config = runner::inproc::python_tool_config(python);
        if matches!(python_config.backend, execenv::BackendKind::Eryx) {
            execenv::prepare_eryx_runtime(python_config).await?;
        }
    }
    let model_config = Arc::new(AgentConfig {
        model_registry: create_model_registry(&config),
        max_iterations: 90,
    });
    let mcp_tools = connect_mcp_servers(&config).await;
    let vfs_provider = config
        .server
        .as_ref()
        .and_then(|s| s.files.as_ref())
        .and_then(|f| f.local.as_ref())
        .filter(|l| l.enabled)
        .map(|l| {
            let keep = config
                .server
                .as_ref()
                .and_then(|s| s.files.as_ref())
                .and_then(|f| f.keep_versions)
                .unwrap_or(10);
            let storage = vfs::LocalFileProvider::new(l.base.clone().into())?;
            Ok(vfs::Vfs::new(
                db.clone(),
                vfs::AnyFileProvider::Local(storage),
                keep,
            ))
        })
        .transpose()
        .map_err(|e: anyhow::Error| e)?
        .map(Arc::new);

    let runner: Arc<dyn runner::AgentPool> = if let Some(ref vfs) = vfs_provider {
        Arc::new(runner::inproc::InProcessAgentPool::with_file_provider(
            db.clone(),
            model_config.clone(),
            tools,
            mcp_tools,
            vfs.clone(),
        ))
    } else {
        Arc::new(runner::inproc::InProcessAgentPool::with_tool_config(
            db.clone(),
            model_config.clone(),
            tools,
            mcp_tools,
        ))
    };

    let state = Arc::new(ServerState {
        config,
        db,
        jwt_secret,
        runner,
        model_config,
        templates,
        vfs: vfs_provider,
    });

    let static_dir = args
        .static_dir
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATIC_DIR));
    let app = app(state, static_dir);

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "server listening");
    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

fn app(state: Arc<ServerState>, static_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .route("/api/logout", post(api::auth::logout))
        .route(
            "/api/projects",
            get(api::projects::list).post(api::projects::create),
        )
        .route(
            "/api/projects/{id}",
            patch(api::projects::rename).delete(api::projects::delete),
        )
        .route(
            "/api/threads",
            get(api::threads::list_threads).post(api::threads::create_thread),
        )
        .route(
            "/api/threads/{id}/messages",
            get(api::threads::list_messages).post(api::threads::send_message),
        )
        .route("/api/threads/{id}/events", get(api::threads::events))
        .route("/api/threads/{id}/cancel", post(api::threads::cancel))
        .route(
            "/api/threads/{id}/files",
            get(api::threads::list_files).post(api::threads::upload_file),
        )
        .route(
            "/api/threads/{id}/directories",
            post(api::threads::create_directory),
        )
        .route(
            "/api/threads/{id}/files/{*path}",
            get(api::threads::download_file).delete(api::threads::delete_file),
        )
        .route("/auth/login", get(pages::auth::login))
        .route("/auth/register", get(pages::auth::register))
        .route("/threads", get(pages::agent::new_thread))
        .route("/threads/{id}", get(pages::agent::thread))
        .route("/", get(root))
        .nest_service("/static", ServeDir::new(static_dir))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

async fn connect_mcp_servers(config: &config::Config) -> Vec<McpTool> {
    let mut tools = Vec::new();

    for (name, server) in &config.mcp {
        let mcp_server = mcp::McpServer {
            url: server.url.clone(),
            headers: server.request_headers(name),
        };
        match mcp::connect(name, mcp_server).await {
            Ok(server_tools) => {
                tracing::info!(server = %name, count = server_tools.len(), "connected to MCP server");
                tools.extend(server_tools);
            }
            Err(error) => {
                tracing::warn!(server = %name, %error, "failed to connect to MCP server");
            }
        }
    }

    tools
}

fn create_model_registry(config: &config::Config) -> ModelRegistry {
    let mut registry = ModelRegistry::new();

    for (name, model) in &config.models {
        let Some(provider) = config.providers.get(&model.provider) else {
            continue;
        };
        let api: API = match provider.kind {
            config::Kind::OpenAI => OpenAI::new(&provider.url).into(),
            config::Kind::Anthropic => Anthropic::new(&provider.url).into(),
            config::Kind::Ollama => Ollama::new(&provider.url).into(),
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

    if !config.models.contains_key(DEFAULT_MODEL)
        && let Some((_, model)) = config.models.iter().next()
        && let Some(provider) = config.providers.get(&model.provider)
    {
        let api: API = match provider.kind {
            config::Kind::OpenAI => OpenAI::new(&provider.url).into(),
            config::Kind::Anthropic => Anthropic::new(&provider.url).into(),
            config::Kind::Ollama => Ollama::new(&provider.url).into(),
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

    registry
}

async fn root(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let path = if is_authenticated(&state, &headers).await {
        "/threads"
    } else {
        "/auth/login"
    };
    Redirect::to(path).into_response()
}

async fn is_authenticated(state: &ServerState, headers: &HeaderMap) -> bool {
    api::auth::authenticated_user(state, headers).await.is_ok()
}
