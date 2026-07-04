mod api;
mod components;
mod config;
mod cron;
mod crypto;
mod db;
mod email;
mod github;
mod google;
mod mcp_servers;
mod notify;
mod pages;
mod rate_limit;
pub mod runner;
mod scheduler;
mod tools;
mod triggers;
mod vfs;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    extract::{DefaultBodyLimit, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, patch, post},
};
use clap::Parser;
use llm::{API, Anthropic, Ollama, OpenAI};
use minisql::ConnectionPool;
use stride_agent::{
    AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry,
    mcp::{self, McpTool},
};
use tower_http::{
    services::ServeDir,
    set_header::SetResponseHeader,
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{rate_limit::RateLimiter, runner::AgentPool};

const UPLOAD_BODY_LIMIT: usize = 25 * 1024 * 1024;
/// Staged uploads are dropped once they are this old without being attached to a
/// thread.
const STAGED_UPLOAD_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the staging area is swept for stale uploads.
const STAGED_UPLOAD_SWEEP: Duration = Duration::from_secs(60 * 60);
const DEFAULT_DEV_JWT_SECRET: &str = "change-this-development-secret";
const MIN_JWT_SECRET_LEN: usize = 32;

const DEFAULT_STATIC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend/dist");

struct ServerState {
    #[allow(dead_code)]
    pub(crate) config: config::Config,
    pub(crate) db: ConnectionPool,
    pub(crate) jwt_secret: String,
    pub(crate) runner: Arc<dyn AgentPool>,
    pub(crate) model_config: Arc<AgentConfig>,
    pub(crate) vfs: Option<Arc<vfs::Vfs>>,
    pub(crate) telegram_interactions: Arc<Mutex<api::telegram::Interactions>>,
    pub(crate) executor: scheduler::ExecutorHandle,
    /// Protects secrets stored at rest, such as linked GitHub access tokens.
    pub(crate) cipher: crypto::SecretCipher,
    /// Present when Google OAuth credentials are configured; drives account
    /// linking and the native Gmail/Calendar/Drive tools.
    pub(crate) google_service: Option<google::GoogleService>,
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
    let jwt_secret = load_jwt_secret()?;

    let db = ConnectionPool::new(&config.db_url()).unwrap();
    db::migrate(&db).await.unwrap();

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
    let encryption_secret = email::encryption_secret(&jwt_secret);
    let cipher = crypto::SecretCipher::new(&encryption_secret);
    let email_service = email::ImapService::new(db.clone(), &encryption_secret);
    let mcp_tools = connect_mcp_servers(&config).await;
    let telegram_bot_token = config
        .server
        .as_ref()
        .and_then(|s| s.telegram.as_ref())
        .and_then(|t| t.read_bot_api_key())
        .filter(|token| !token.is_empty());
    // The hosted GitHub MCP server is offered to every user who has linked an
    // account, whether through the OAuth App or a Personal Access Token. The
    // runtime only needs the endpoint and the cipher, so it is always available;
    // the per-user connection lookup decides whether any tools are attached.
    let github_mcp_url = config
        .server
        .as_ref()
        .and_then(|s| s.github.as_ref())
        .map(|github| github.mcp_url().to_string())
        .unwrap_or_else(|| github::DEFAULT_MCP_URL.to_string());
    let github_runtime = Some(github::GitHubRuntime {
        mcp_url: github_mcp_url,
        cipher: cipher.clone(),
    });
    // Google account linking and native Gmail/Calendar/Drive tools, active once
    // OAuth credentials are present.
    let google_service = api::google::build_service(&config, &db, &cipher);
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

    let executor = scheduler::spawn(
        db.clone(),
        model_config.clone(),
        tools.clone(),
        telegram_bot_token.clone(),
        email_service.clone(),
        mcp_tools.clone(),
        google_service.clone(),
    );

    let public_url = config.public_url();
    let runner: Arc<dyn runner::AgentPool> = if let Some(ref vfs) = vfs_provider {
        Arc::new(
            runner::inproc::InProcessAgentPool::with_file_provider_and_telegram(
                db.clone(),
                model_config.clone(),
                tools,
                mcp_tools,
                vfs.clone(),
                telegram_bot_token.clone(),
                public_url,
                github_runtime.clone(),
                email_service.clone(),
                google_service.clone(),
            ),
        )
    } else {
        Arc::new(
            runner::inproc::InProcessAgentPool::with_tool_config_and_telegram(
                db.clone(),
                model_config.clone(),
                tools,
                mcp_tools,
                telegram_bot_token.clone(),
                public_url,
                github_runtime.clone(),
                email_service,
                google_service.clone(),
            ),
        )
    };

    let state = Arc::new(ServerState {
        config,
        db,
        jwt_secret,
        runner,
        model_config,
        vfs: vfs_provider,
        telegram_interactions: Arc::new(Mutex::new(api::telegram::Interactions::default())),
        executor,
        cipher,
        google_service,
    });

    // Bind Telegram subscriber tasks to agent runner lifetimes (created on activation, aborted on
    // eviction) so per-thread forwarders do not accumulate.
    tokio::spawn(api::telegram::supervise(state.clone()));

    // Sweep stale staged uploads so files attached to a thread that was never
    // created do not accumulate forever.
    if state.vfs.is_some() {
        tokio::spawn(sweep_staged_uploads(state.clone()));
    }

    // Register the webhook with callback_query updates enabled so inline button taps are delivered.
    if let Some(token) = telegram_bot_token {
        let telegram = state
            .config
            .server
            .as_ref()
            .and_then(|s| s.telegram.as_ref());
        match telegram.and_then(|t| t.read_webhook_url()) {
            Some(url) => {
                let secret = telegram.and_then(|t| t.read_webhook_secret());
                api::telegram::register_webhook(token, url, secret).await;
            }
            None => tracing::warn!(
                "telegram.webhook_url is not configured; skipping setWebhook. Inline button taps \
                 (approvals/quizzes) will not be delivered unless the webhook was registered \
                 externally with allowed_updates including callback_query"
            ),
        }
    }

    let static_dir = args
        .static_dir
        .or_else(|| std::env::var_os("STRIDE_STATIC_DIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATIC_DIR));
    let app = app(state, static_dir);

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn sweep_staged_uploads(state: Arc<ServerState>) {
    let Some(vfs) = state.vfs.clone() else {
        return;
    };
    let mut interval = tokio::time::interval(STAGED_UPLOAD_SWEEP);
    loop {
        interval.tick().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let cutoff = now - STAGED_UPLOAD_TTL.as_millis() as i64;
        match vfs.cleanup_staged_uploads(cutoff).await {
            Ok(removed) if removed > 0 => {
                tracing::info!(removed, "swept stale staged uploads")
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "failed to sweep staged uploads"),
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

fn load_jwt_secret() -> anyhow::Result<String> {
    let secret = std::env::var("STRIDE_JWT_SECRET").map_err(|_| {
        anyhow::anyhow!(
            "STRIDE_JWT_SECRET is not set. Set a strong, random STRIDE_JWT_SECRET (at least \
             {MIN_JWT_SECRET_LEN} bytes) before starting the server."
        )
    })?;

    if secret == DEFAULT_DEV_JWT_SECRET {
        anyhow::bail!(
            "STRIDE_JWT_SECRET is set to the insecure default. Set a strong, random \
             STRIDE_JWT_SECRET (at least {MIN_JWT_SECRET_LEN} bytes) before starting the server."
        );
    }

    if secret.len() < MIN_JWT_SECRET_LEN {
        anyhow::bail!(
            "STRIDE_JWT_SECRET is too short ({} bytes). Set a strong, random STRIDE_JWT_SECRET \
             with at least {MIN_JWT_SECRET_LEN} bytes before starting the server.",
            secret.len()
        );
    }

    if std::env::var("STRIDE_EMAIL_ENCRYPTION_KEY").is_err() {
        tracing::warn!(
            "STRIDE_EMAIL_ENCRYPTION_KEY is not set; stored IMAP passwords fall back to the JWT \
             secret for encryption. A dedicated STRIDE_EMAIL_ENCRYPTION_KEY is recommended."
        );
    }

    Ok(secret)
}

fn app(state: Arc<ServerState>, static_dir: PathBuf) -> Router {
    let limiter = Arc::new(RateLimiter::new());

    let auth_routes = Router::new()
        .route("/api/register", post(api::auth::register))
        .route("/api/login", post(api::auth::login))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            rate_limit::limit,
        ));

    Router::new()
        .merge(auth_routes)
        .route("/api/logout", post(api::auth::logout))
        .route("/v1/models", get(api::openai::list_models))
        .route("/v1/models/{*model}", get(api::openai::get_model))
        .route("/v1/chat/completions", post(api::openai::chat_completion))
        .route("/api/settings/telegram", get(api::telegram::settings))
        .route("/api/settings/github", get(api::github::settings))
        .route(
            "/api/settings/github/authorize",
            get(api::github::authorize),
        )
        .route("/api/settings/github/callback", get(api::github::callback))
        .route("/api/settings/github/pat", post(api::github::connect_pat))
        .route(
            "/api/settings/github/disconnect",
            post(api::github::disconnect),
        )
        .route("/api/settings/google", get(api::google::settings))
        .route(
            "/api/settings/google/authorize",
            get(api::google::authorize),
        )
        .route("/api/settings/google/callback", get(api::google::callback))
        .route(
            "/api/settings/google/disconnect",
            post(api::google::disconnect),
        )
        .route(
            "/api/settings/mcp",
            get(api::mcp::list).post(api::mcp::create),
        )
        .route("/api/settings/mcp/{id}", delete(api::mcp::delete))
        .route(
            "/api/settings/email",
            get(api::email::list).post(api::email::create),
        )
        .route("/api/settings/email/{id}", delete(api::email::delete))
        .route(
            "/api/settings/skills",
            get(api::skills::list).post(api::skills::create),
        )
        .route(
            "/api/settings/skills/{id}",
            patch(api::skills::update).delete(api::skills::delete),
        )
        .route(
            "/api/settings/writable-dirs",
            get(api::writable_dirs::list).post(api::writable_dirs::create),
        )
        .route(
            "/api/settings/writable-dirs/{id}",
            delete(api::writable_dirs::delete),
        )
        .route("/api/settings/memories", get(api::memories::list))
        .route("/api/settings/memories/{id}", delete(api::memories::delete))
        .route("/api/settings/telegram/login", post(api::telegram::login))
        .route(
            "/api/settings/telegram/disconnect",
            post(api::telegram::disconnect),
        )
        .route("/api/telegram/webhook", post(api::telegram::webhook))
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
            "/api/threads/{id}/approvals/{approval_id}",
            post(api::threads::resolve_approval),
        )
        .route(
            "/api/threads/{id}/quizzes/{quiz_id}",
            post(api::threads::answer_quiz),
        )
        .route(
            "/api/threads/{id}/files",
            get(api::threads::list_files)
                .post(api::threads::upload_file)
                .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/api/threads/{id}/directories",
            post(api::threads::create_directory),
        )
        .route(
            "/api/threads/{id}/files/{*path}",
            get(api::threads::download_file).delete(api::threads::delete_file),
        )
        .route(
            "/api/uploads",
            post(api::uploads::upload).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/api/transcribe",
            post(api::transcribe::transcribe).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/api/files",
            get(api::files::list_files)
                .post(api::files::upload_file)
                .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/api/files/directories", post(api::files::create_directory))
        .route("/api/files/rename", patch(api::files::rename))
        .route(
            "/api/automations",
            get(api::automations::list).post(api::automations::create),
        )
        .route(
            "/api/automations/{id}",
            patch(api::automations::update).delete(api::automations::delete),
        )
        .route("/api/automations/{id}/runs", get(api::automations::runs))
        .route("/api/automations/{id}/run", post(api::automations::run_now))
        .route(
            "/api/automations/{id}/webhook",
            post(api::automations::webhook),
        )
        .route(
            "/api/files/{*path}",
            get(api::files::download_file).delete(api::files::delete_file),
        )
        .route("/api/public/images/{token}", get(api::images::serve))
        .route("/auth/login", get(pages::auth::login))
        .route("/auth/register", get(pages::auth::register))
        .route("/threads", get(pages::agent::new_thread))
        .route("/threads/{id}", get(pages::agent::thread))
        .route("/files", get(pages::files::files))
        .route("/automations", get(pages::automations::automations))
        .route("/settings", get(pages::settings::settings))
        .route("/", get(root))
        .nest_service(
            "/static",
            SetResponseHeader::if_not_present(
                ServeDir::new(static_dir),
                axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                axum::http::HeaderValue::from_static("*"),
            ),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "request",
                        method = %req.method(),
                        path = %req.uri().path(),
                    )
                })
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
            config::Kind::OpenRouter => OpenAI::openrouter(&provider.url).into(),
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
                reasoning_effort: model.reasoning_effort(),
                vision: model.vision.unwrap_or(false),
            },
        );
    }

    if !config.models.contains_key(DEFAULT_MODEL)
        && let Some((_, model)) = config.models.iter().next()
        && let Some(provider) = config.providers.get(&model.provider)
    {
        let api: API = match provider.kind {
            config::Kind::OpenAI => OpenAI::new(&provider.url).into(),
            config::Kind::OpenRouter => OpenAI::openrouter(&provider.url).into(),
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
                reasoning_effort: model.reasoning_effort(),
                vision: model.vision.unwrap_or(false),
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
