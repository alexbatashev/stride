use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use minisql::{ConnectionPool, Value};
use stride_agent::{
    AgentConfig, BaseAgent, Tool, ToolRegistry,
    mcp::McpTool,
    tools::{
        email::{CreateEmailDraftTool, ListEmailsTool},
        firecrawl::FirecrawlTool,
        quiz::QuizTool,
        shell::ShellTool,
        subagent::{SUBAGENT_NAME, SubAgentTool},
        web_search::{
            WebSearchTool, arxiv::ArxivProvider, brave::BraveProvider, pubmed::PubmedProvider,
            searxng::SearxngProvider, uspto::UsptoProvider,
        },
    },
};
use uuid::Uuid;

use crate::{
    config::{self, Firecrawl, Python, PythonBackend, PythonNetwork, Tools, WebSearch},
    crypto::SecretCipher,
    db::MessageFormat,
    email::ImapService,
    github::GitHubRuntime,
    google::GoogleService,
    model_registry,
    runner::{
        AgentPoolError, RUNNER_LIFECYCLE_TOPIC, RunnerLifecycle, db_error,
        pool::{PoolHandle, ThreadRunner, WorkerState},
    },
    tools::{
        attach_image::AttachImageTool,
        automations::ScheduleAutomationTool,
        memory::{ConnectMemoriesTool, ExplorePalaceTool, RecallTool, RememberTool},
        ocr::OcrTool,
        personality::UpdatePersonalityTool,
        projects::{CreateProjectTool, ListProjectsTool, StartThreadTool},
        python::VfsExecFileSystem,
        shell::EmulatedShellBackend,
        skills::{CreateSkillTool, LoadSkillTool, SearchSkillsTool},
        telegram::{SendTelegramFileTool, SendTelegramMessageTool},
    },
    vfs::{MountedVfs, Vfs, WritableArea},
};

use super::prompt::build_system_prompt;

/// Everything `ensure_runner` needs out of the worker's shared init, cloned once up front so the
/// rest of the function borrows nothing from `state`. Replaces a positional 14-tuple.
struct WorkerDeps {
    db: ConnectionPool,
    config: Arc<AgentConfig>,
    server_config: config::Config,
    cipher: SecretCipher,
    tools: Tools,
    mcp_tools: Vec<McpTool>,
    vfs: Option<Arc<Vfs>>,
    telegram_bot_token: Option<String>,
    public_url: Option<String>,
    github_runtime: Option<GitHubRuntime>,
    email_service: Option<ImapService>,
    google_service: Option<GoogleService>,
    base_system_prompt: String,
    pool: PoolHandle,
}

impl WorkerDeps {
    fn from_state(state: &Rc<RefCell<WorkerState>>) -> Self {
        let state = state.borrow();
        WorkerDeps {
            db: state.init.db.clone(),
            config: state.init.config.clone(),
            server_config: state.init.server_config.clone(),
            cipher: state.init.cipher.clone(),
            tools: state.init.tools.clone(),
            mcp_tools: state.init.mcp_tools.clone(),
            vfs: state.init.vfs.clone(),
            telegram_bot_token: state.init.telegram_bot_token.clone(),
            public_url: state.init.public_url.clone(),
            github_runtime: state.init.github_runtime.clone(),
            email_service: state.init.email_service.clone(),
            google_service: state.init.google_service.clone(),
            base_system_prompt: state.init.system_prompt.clone(),
            pool: state.pool.clone(),
        }
    }
}

pub(crate) async fn ensure_runner(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Result<(), AgentPoolError> {
    if state.borrow().threads.contains_key(&thread_id) {
        return Ok(());
    }

    let WorkerDeps {
        db,
        config,
        server_config,
        cipher,
        tools,
        mcp_tools,
        vfs,
        telegram_bot_token,
        public_url,
        github_runtime,
        email_service,
        google_service,
        base_system_prompt,
        pool,
    } = WorkerDeps::from_state(&state);

    ensure_thread_exists(&db, thread_id).await?;
    let user_id = thread_owner(&db, thread_id).await?;
    let merged_registry = model_registry::build_user_registry(
        &server_config,
        &config.model_registry,
        &db,
        user_id,
        &cipher,
    )
    .await
    .map_err(|error| AgentPoolError::Internal(anyhow::anyhow!(error)))?;
    let agent_settings = model_registry::load_agent_settings(&server_config, &db, user_id)
        .await
        .map_err(|error| AgentPoolError::Internal(anyhow::anyhow!(error)))?;
    let config = Arc::new(AgentConfig {
        model_registry: merged_registry,
        max_iterations: config.max_iterations,
        usage_observer: config.usage_observer.clone(),
        clock: config.clock.clone(),
        id_gen: config.id_gen.clone(),
        max_concurrent_tools: config.max_concurrent_tools,
    });
    let vision = config.model_registry.get_or_default("default").vision;
    let mut mcp_tools = mcp_tools;
    mcp_tools.extend(crate::mcp_servers::connect_user_mcp_servers(&db, user_id).await);
    if let Some(github_runtime) = github_runtime.as_ref() {
        mcp_tools
            .extend(crate::github::connect_user_github_mcp(&db, user_id, github_runtime).await);
    }
    // Offer the native Google tools only when this user has actually linked an
    // account; an unlinked user sees none of them.
    let google_for_tools = match google_service {
        Some(service) if service.is_connected(user_id).await => Some(service),
        _ => None,
    };
    let project_id = thread_project_id(&db, thread_id).await?;
    // Resolve where this thread writes: a project thread writes into the
    // project's folder in the user's global files; an ungrouped thread keeps a
    // standalone `/~workspace`.
    let (writable_area, project_title) = match vfs.as_ref() {
        Some(vfs) => {
            let (area, title) =
                resolve_writable_area(&db, vfs, thread_id, project_id, user_id).await?;
            (Some(area), title)
        }
        None => (None, None),
    };
    let writable_root = writable_area.as_ref().map(writable_root_path);
    // Personal directories the user marked writable, layered on top of the
    // thread's own workspace or project folder.
    let writable_extra = if writable_area.is_some() {
        crate::api::writable_dirs::writable_prefixes(&db, user_id).await
    } else {
        Vec::new()
    };
    let personality = load_personality(&db, user_id).await?;
    // A thread bound to a Telegram chat enables the file-delivery tool and absolute download links.
    let telegram_chat = if telegram_bot_token.is_some() {
        crate::tools::telegram::thread_chat(&db, thread_id)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let message_format = if telegram_chat.is_some() {
        MessageFormat::Markdown
    } else {
        MessageFormat::Html
    };
    let mut system_prompt = build_system_prompt(
        &base_system_prompt,
        personality.as_deref(),
        vfs.as_ref().map(|_| thread_id),
        writable_root.as_deref(),
        &writable_extra,
        telegram_chat.is_some(),
        public_url.as_deref(),
        config.clock.as_ref(),
    );
    let excluded_static_skills = if telegram_chat.is_some() {
        vec!["inline-widget".to_string()]
    } else {
        Vec::new()
    };
    let catalog = crate::tools::skills::skill_catalog(&db, user_id, &excluded_static_skills).await;
    if !catalog.is_empty() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&catalog);
    }
    system_prompt.push_str("\n\n");
    system_prompt
        .push_str(&crate::tools::memory::palace_map(&db, user_id, project_title.as_deref()).await);
    let (thread, next_message_seq) = load_thread(&db, thread_id).await?;
    // Resume the per-thread event counter from the journal so seq never resets on
    // runner recreation; a fresh thread with no journaled events starts at 0.
    let last_event_seq = super::inproc::load_last_event_seq(&db, thread_id).await;
    let agent = BaseAgent::new(
        stride_agent::DEFAULT_MODEL.to_string(),
        config.clone(),
        system_prompt,
        thread,
    );
    agent.set_searchable_tools_preview_limit(server_config.searchable_tools_preview_limit());
    configure_agent_tools(
        &agent,
        &tools,
        &agent_settings.subagent_allowed_models,
        &agent_settings.subagent_guidelines,
    );
    // The Python sandbox tool set is built from the same place as scheduled
    // automations (`scriptable_tool_registry`), so scripts behave identically in
    // the interactive loop and on cron. Built before `mcp_tools`/`email_service`
    // are consumed below.
    let python_enabled = tools
        .python
        .as_ref()
        .is_some_and(|python| python.enabled != Some(false));
    let python_tools = python_enabled.then(|| {
        scriptable_tool_registry(ScriptableToolRegistryContext {
            tools: &tools,
            db: &db,
            user_id,
            email_provider: email_service
                .as_ref()
                .map(|service| service.provider(user_id)),
            mcp_tools: &mcp_tools,
            default_wing: project_title.clone(),
            google: google_for_tools.clone().map(|service| (service, user_id)),
        })
    });
    for tool in mcp_tools {
        agent.register_searchable_tool(tool);
    }
    agent.register_tool(UpdatePersonalityTool {
        db: db.clone(),
        user_id,
    });
    agent.allow_tool("update_personality");
    agent.register_tool(ScheduleAutomationTool {
        db: db.clone(),
        user_id,
    });
    agent.allow_tool("schedule_automation");
    // Project and thread management, hidden by default and reachable via search_tools.
    agent.register_searchable_tool(CreateProjectTool {
        db: db.clone(),
        user_id,
        vfs: vfs.clone(),
    });
    agent.allow_tool("create_project");
    agent.register_searchable_tool(ListProjectsTool {
        db: db.clone(),
        user_id,
    });
    agent.allow_tool("list_projects");
    agent.register_searchable_tool(StartThreadTool {
        db: db.clone(),
        user_id,
        pool: pool.clone(),
    });
    agent.allow_tool("start_thread");
    agent.register_tool(SearchSkillsTool {
        db: db.clone(),
        user_id,
        excluded_static_skills: excluded_static_skills.clone(),
    });
    agent.allow_tool("search_skills");
    agent.register_tool(LoadSkillTool {
        db: db.clone(),
        user_id,
        excluded_static_skills,
    });
    agent.allow_tool("load_skill");
    agent.register_tool(CreateSkillTool {
        db: db.clone(),
        user_id,
    });
    agent.allow_tool("create_skill");
    agent.register_tool(RememberTool {
        db: db.clone(),
        user_id,
        default_wing: project_title.clone(),
    });
    agent.allow_tool("remember");
    agent.register_tool(RecallTool {
        db: db.clone(),
        user_id,
        default_wing: project_title.clone(),
    });
    agent.allow_tool("recall");
    agent.register_tool(ExplorePalaceTool {
        db: db.clone(),
        user_id,
    });
    agent.allow_tool("explore_palace");
    agent.register_tool(ConnectMemoriesTool {
        db: db.clone(),
        user_id,
    });
    agent.allow_tool("connect_memories");
    if let Some(email_service) = email_service {
        let provider = email_service.provider(user_id);
        agent.register_searchable_tool(ListEmailsTool {
            provider: provider.clone(),
        });
        agent.allow_tool("list_emails");
        agent.register_searchable_tool(CreateEmailDraftTool { provider });
        agent.allow_tool("create_email_draft");
    }
    if let Some(service) = google_for_tools {
        crate::tools::google::register(&agent, service, user_id);
    }
    if let Some(bot_token) = telegram_bot_token.clone() {
        agent.register_tool(SendTelegramMessageTool {
            db: db.clone(),
            user_id,
            thread_id,
            bot_token,
        });
        agent.allow_tool("send_telegram_message");
    }
    let python_workspace = match (vfs, writable_area) {
        (Some(provider), Some(area)) => Some((provider, area)),
        _ => None,
    };

    // Build the Python interpreter first so the shell can expose it as a
    // `python` command sharing the same runtime and workspace.
    let python = python_tool(
        &tools,
        thread_id,
        python_workspace.clone(),
        writable_extra.clone(),
        user_id,
    )
    .await
    .map_err(AgentPoolError::Internal)?;

    if let Some((provider, area)) = python_workspace {
        let fs = MountedVfs::new(provider.clone(), user_id, area.clone())
            .with_writable_dirs(writable_extra.clone());
        if vision {
            agent.register_tool(AttachImageTool {
                fs: fs.clone(),
                vfs: provider.clone(),
                db: db.clone(),
                owner: user_id,
                public_url: public_url.clone(),
            });
            agent.allow_tool("attach_image");
        }
        agent.register_tool(OcrTool {
            fs: fs.clone(),
            python: python.as_ref().map(|tool| tool.service()),
            writable_root: writable_root
                .clone()
                .unwrap_or_else(|| format!("/{}", crate::vfs::WORKSPACE_MOUNT)),
        });
        agent.allow_tool("ocr");
        if let Some(bot_token) = telegram_bot_token
            .as_ref()
            .filter(|_| telegram_chat.is_some())
        {
            agent.register_tool(SendTelegramFileTool {
                db: db.clone(),
                fs: fs.clone(),
                user_id,
                thread_id,
                bot_token: bot_token.clone(),
            });
            agent.allow_tool("send_telegram_file");
        }
        let mut shell = EmulatedShellBackend::new(fs);
        if let Some(tool) = &python {
            shell = shell.with_python(tool.service());
        }
        let python_cfg = tools
            .python
            .as_ref()
            .map(python_tool_config)
            .unwrap_or_default();
        shell = shell.with_typst(
            Some(python_cfg.cache_dir.join("typst-packages")),
            vec![python_cfg.cache_dir.join("fonts")],
            matches!(python_cfg.network, execenv::NetworkAccess::Allowed),
        );
        agent.register_tool(ShellTool::new(shell));
    }

    if let (Some(tool), Some(registry)) = (python, python_tools) {
        agent.register_tool(tool.with_tools(registry));
        agent.allow_tool("python");
    }

    state.borrow_mut().threads.insert(
        thread_id,
        ThreadRunner {
            owner: user_id,
            agent: Some(agent),
            cancel_tx: None,
            broker: Arc::new(stride_agent::InMemoryInteractionBroker::default()),
            queued: std::collections::VecDeque::new(),
            last_event_seq,
            next_message_seq,
            status: crate::runner::ThreadStatus::Idle,
            in_progress: None,
            message_format,
            last_used: config.clock.now_instant(),
        },
    );

    // Announce the new runner so the Telegram supervisor can bind a subscriber task to its
    // lifetime. Published for every thread; the supervisor filters to Telegram-mapped ones.
    let _ = pubsub::topic::<RunnerLifecycle>(RUNNER_LIFECYCLE_TOPIC)
        .publish(&RunnerLifecycle::Activated { thread_id });

    Ok(())
}

fn configure_agent_tools(
    agent: &BaseAgent,
    tools: &Tools,
    allowed_models: &[String],
    guidelines: &str,
) {
    agent.register_tool(QuizTool);

    agent.register_tool(SubAgentTool::new(
        subagent_tool_registry(tools),
        allowed_models.to_vec(),
        guidelines,
    ));
    agent.allow_tool(SUBAGENT_NAME);

    if let Some(web_search) = &tools.web_search {
        agent.register_tool(web_search_tool(web_search));
    }

    if let Some(firecrawl) = &tools.firecrawl
        && let Some(tool) = firecrawl_tool(firecrawl)
    {
        agent.register_tool(tool);
    }
}

async fn python_tool(
    tools: &Tools,
    thread_id: Uuid,
    workspace: Option<(Arc<Vfs>, WritableArea)>,
    writable_extra: Vec<String>,
    user_id: Uuid,
) -> anyhow::Result<Option<execenv::PythonTool>> {
    let Some(python) = tools.python.as_ref() else {
        return Ok(None);
    };
    if python.enabled == Some(false) {
        return Ok(None);
    }

    let config = python_tool_config(python);
    let cache_dir = config.cache_dir.clone();
    let fs: Arc<dyn execenv::FileSystemBackend> = if let Some((vfs, area)) = workspace {
        Arc::new(VfsExecFileSystem::new(
            vfs,
            area,
            writable_extra,
            user_id,
            cache_dir
                .join("workspaces")
                .join(thread_id.as_simple().to_string()),
        ))
    } else {
        Arc::new(execenv::DirectOsFileSystem::new(
            cache_dir
                .join("workspaces")
                .join(thread_id.as_simple().to_string()),
        )?)
    };

    execenv::PythonTool::new(config, fs).await.map(Some)
}

pub(crate) fn python_tool_config(python: &Python) -> execenv::PythonToolConfig {
    let mut config = execenv::PythonToolConfig::default();
    if let Some(cache_dir) = python.cache_dir.as_ref() {
        config.cache_dir = PathBuf::from(cache_dir);
    }
    config.backend = match python.backend.as_ref().unwrap_or(&PythonBackend::Eryx) {
        PythonBackend::Mock => execenv::BackendKind::Mock,
        PythonBackend::Eryx => execenv::BackendKind::Eryx,
    };
    config.threads = python.threads.unwrap_or(1);
    config.preinit = python.preinit.unwrap_or(true);
    config.limits = execenv::ExecutionLimits {
        max_runtime: Duration::from_secs(python.max_runtime_seconds.unwrap_or(30)),
        max_memory_bytes: python.max_memory_bytes.or(Some(128 * 1024 * 1024)),
        max_cpu_fuel: python.max_cpu_fuel,
    };
    config.network = match python.network.as_ref().unwrap_or(&PythonNetwork::Blocked) {
        PythonNetwork::Blocked => execenv::NetworkAccess::Blocked,
        PythonNetwork::Allowed => execenv::NetworkAccess::Allowed,
    };
    config
}

pub(crate) fn subagent_tool_registry(tools: &Tools) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    if let Some(web_search) = &tools.web_search {
        let tool = web_search_tool(web_search);
        registry.allow_tool(tool.name());
        registry.register(tool);
    }

    if let Some(firecrawl) = &tools.firecrawl
        && let Some(tool) = firecrawl_tool(firecrawl)
    {
        registry.allow_tool(tool.name());
        registry.register(tool);
    }

    registry
}

/// The single source of truth for the tools a Python script can call through the
/// `tools` package. Both the interactive agent loop and scheduled automations
/// build their sandbox tool set from here, so a script behaves identically in
/// either mode. Only read-only MCP tools are exposed; state-changing ones need
/// interactive approval that a script cannot provide, so they are left out.
pub(crate) struct ScriptableToolRegistryContext<'a> {
    pub tools: &'a Tools,
    pub db: &'a ConnectionPool,
    pub user_id: Uuid,
    pub email_provider: Option<Arc<dyn stride_agent::tools::email::EmailProvider>>,
    pub mcp_tools: &'a [McpTool],
    pub default_wing: Option<String>,
    pub google: Option<(GoogleService, Uuid)>,
}

pub(crate) fn scriptable_tool_registry(ctx: ScriptableToolRegistryContext<'_>) -> ToolRegistry {
    let mut registry = subagent_tool_registry(ctx.tools);

    if let Some(provider) = ctx.email_provider {
        registry.register(ListEmailsTool {
            provider: provider.clone(),
        });
        registry.allow_tool("list_emails");
        registry.register(CreateEmailDraftTool { provider });
        registry.allow_tool("create_email_draft");
    }

    if let Some((service, user)) = ctx.google {
        crate::tools::google::register_scriptable(&mut registry, service, user);
    }

    registry.register(RememberTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
        default_wing: ctx.default_wing.clone(),
    });
    registry.allow_tool("remember");
    registry.register(RecallTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
        default_wing: ctx.default_wing,
    });
    registry.allow_tool("recall");
    registry.register(ExplorePalaceTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
    });
    registry.allow_tool("explore_palace");
    registry.register(ConnectMemoriesTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
    });
    registry.allow_tool("connect_memories");

    registry.register(SearchSkillsTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
        excluded_static_skills: Vec::new(),
    });
    registry.allow_tool("search_skills");
    registry.register(LoadSkillTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
        excluded_static_skills: Vec::new(),
    });
    registry.allow_tool("load_skill");
    registry.register(CreateSkillTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
    });
    registry.allow_tool("create_skill");

    registry.register(UpdatePersonalityTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
    });
    registry.allow_tool("update_personality");

    registry.register(ScheduleAutomationTool {
        db: ctx.db.clone(),
        user_id: ctx.user_id,
    });
    registry.allow_tool("schedule_automation");

    for tool in ctx.mcp_tools {
        if tool.requires_confirmation() {
            continue;
        }
        registry.register_searchable(tool.clone());
    }

    registry
}

fn web_search_tool(web_search: &WebSearch) -> WebSearchTool {
    let mut providers: Vec<Box<dyn stride_agent::tools::web_search::SearchProvider>> =
        vec![Box::new(SearxngProvider {
            endpoint: web_search.searxng_endpoint.clone(),
            request_delay: Duration::from_secs(
                web_search.searxng_request_delay_seconds.unwrap_or(5),
            ),
        })];

    if let Some(api_key) = web_search.read_brave_api_key() {
        providers.push(Box::new(BraveProvider {
            api_key,
            endpoint: web_search.brave_endpoint().to_string(),
        }));
    }

    if web_search.include_arxiv == Some(true) {
        providers.push(Box::new(ArxivProvider));
    }

    if web_search.include_pubmed == Some(true) {
        providers.push(Box::new(PubmedProvider));
    }

    if web_search.include_uspto == Some(true) {
        providers.push(Box::new(UsptoProvider));
    }

    WebSearchTool {
        providers,
        ranker: Box::new(stride_agent::tools::web_search::InterleaveRanker),
    }
}

fn firecrawl_tool(firecrawl: &Firecrawl) -> Option<FirecrawlTool> {
    firecrawl.read_api_key().map(|api_key| FirecrawlTool {
        api_key,
        api_url: firecrawl.api_url().to_string(),
    })
}

async fn thread_project_id(
    db: &ConnectionPool,
    thread_id: Uuid,
) -> Result<Option<Uuid>, AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT project_id FROM threads WHERE id = ? LIMIT 1",
            vec![Value::Uuid(thread_id)],
        )
        .await
        .map_err(db_error)?;

    Ok(result
        .rows()
        .first()
        .and_then(|row| match row.get("project_id") {
            Some(Value::Uuid(id)) => Some(*id),
            Some(Value::Blob(bytes)) if bytes.len() == 16 => Uuid::from_slice(bytes).ok(),
            Some(Value::Text(s)) => Uuid::parse_str(s).ok(),
            _ => None,
        }))
}

/// Determines a thread's writable area. A thread bound to a project writes into
/// that project's folder in the owner's global files; an ungrouped thread keeps
/// its own standalone workspace. Also returns the project title when present, so
/// callers can default the memory wing and prompt to it.
async fn resolve_writable_area(
    db: &ConnectionPool,
    vfs: &Vfs,
    thread_id: Uuid,
    project_id: Option<Uuid>,
    owner: Uuid,
) -> Result<(WritableArea, Option<String>), AgentPoolError> {
    if let Some(pid) = project_id
        && let Some(title) = project_title(db, pid).await?
    {
        let prefix = vfs
            .ensure_project_dir(owner, &title)
            .await
            .map_err(AgentPoolError::Internal)?;
        return Ok((WritableArea::ProjectDir(prefix), Some(title)));
    }

    let workspace_id = vfs
        .get_or_create_workspace(thread_id, None, owner)
        .await
        .map_err(AgentPoolError::Internal)?;
    Ok((WritableArea::Workspace(workspace_id), None))
}

/// The absolute path the agent uses to reach its writable directory.
fn writable_root_path(area: &WritableArea) -> String {
    match area {
        WritableArea::Workspace(_) => format!("/{}", crate::vfs::WORKSPACE_MOUNT),
        WritableArea::ProjectDir(prefix) => format!("/{prefix}"),
    }
}

async fn project_title(
    db: &ConnectionPool,
    project_id: Uuid,
) -> Result<Option<String>, AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT title FROM projects WHERE id = ? LIMIT 1",
            vec![Value::Uuid(project_id)],
        )
        .await
        .map_err(db_error)?;

    Ok(result
        .rows()
        .first()
        .and_then(|row| row.get_text("title"))
        .map(|s| s.to_string()))
}

async fn thread_owner(db: &ConnectionPool, thread_id: Uuid) -> Result<Uuid, AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT owner FROM threads WHERE id = ? LIMIT 1",
            vec![Value::Uuid(thread_id)],
        )
        .await
        .map_err(db_error)?;

    result
        .rows()
        .first()
        .and_then(|row| match row.get("owner") {
            Some(Value::Uuid(id)) => Some(*id),
            Some(Value::Blob(bytes)) if bytes.len() == 16 => Uuid::from_slice(bytes).ok(),
            Some(Value::Text(s)) => Uuid::parse_str(s).ok(),
            _ => None,
        })
        .ok_or_else(|| AgentPoolError::Internal(anyhow::anyhow!("thread owner not found")))
}

async fn load_personality(
    db: &ConnectionPool,
    user_id: Uuid,
) -> Result<Option<String>, AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT personality FROM users WHERE id = ? LIMIT 1",
            vec![Value::Uuid(user_id)],
        )
        .await
        .map_err(db_error)?;

    Ok(result
        .rows()
        .first()
        .and_then(|row| row.get_text("personality"))
        .map(|s| s.to_string()))
}

async fn ensure_thread_exists(db: &ConnectionPool, thread_id: Uuid) -> Result<(), AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT id FROM threads WHERE id = ? LIMIT 1",
            vec![Value::Uuid(thread_id)],
        )
        .await
        .map_err(db_error)?;

    if result.is_empty() {
        Err(AgentPoolError::ThreadNotFound)
    } else {
        Ok(())
    }
}

pub(crate) async fn load_thread(
    db: &ConnectionPool,
    thread_id: Uuid,
) -> Result<(Vec<llm::Message>, u64), AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT seq, role, content, images, thinking, tool_calls, tool_call_id FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
            vec![Value::Uuid(thread_id)],
        )
        .await
        .map_err(db_error)?;

    let mut thread = Vec::new();
    let mut next_seq = 0;

    for row in result.rows() {
        let seq = row
            .get_int("seq")
            .and_then(|seq| u64::try_from(seq).ok())
            .ok_or_else(|| AgentPoolError::Internal(anyhow::anyhow!("invalid message seq")))?;
        let role = match row.get_text("role") {
            Some("system") => llm::Role::System,
            Some("agent") => llm::Role::Assistant,
            Some("user") => llm::Role::User,
            Some("tool") => llm::Role::Tool,
            Some(role) => {
                return Err(AgentPoolError::Internal(anyhow::anyhow!(
                    "invalid message role: {role}"
                )));
            }
            None => {
                return Err(AgentPoolError::Internal(anyhow::anyhow!(
                    "missing message role"
                )));
            }
        };

        thread.push(llm::Message {
            role,
            content: row.get_text("content").unwrap_or_default().to_string(),
            images: match row.get("images") {
                Some(Value::Text(images)) => serde_json::from_str(images)
                    .map_err(|error| AgentPoolError::Internal(anyhow::anyhow!(error)))?,
                _ => None,
            },
            thinking: match row.get("thinking") {
                Some(Value::Text(thinking)) => Some(thinking.clone()),
                _ => None,
            },
            tool_calls: match row.get("tool_calls") {
                Some(Value::Text(tool_calls)) => Some(
                    serde_json::from_str(tool_calls)
                        .map_err(|error| AgentPoolError::Internal(anyhow::anyhow!(error)))?,
                ),
                _ => None,
            },
            tool_call_id: match row.get("tool_call_id") {
                Some(Value::Text(tool_call_id)) => Some(tool_call_id.clone()),
                _ => None,
            },
        });

        next_seq = seq + 1;
    }

    Ok((thread, next_seq))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Firecrawl, WebSearch};
    use stride_agent::{AgentConfig, ModelRegistry};

    #[test]
    fn configured_tools_are_registered_on_agent() {
        let agent = BaseAgent::new(
            "default".to_string(),
            Arc::new(AgentConfig {
                model_registry: ModelRegistry::new(),
                max_iterations: 0,
                usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                ..Default::default()
            }),
            "System prompt".to_string(),
            Vec::new(),
        );

        configure_agent_tools(
            &agent,
            &Tools {
                web_search: Some(WebSearch {
                    searxng_endpoint: "https://search.example.com".to_string(),
                    searxng_request_delay_seconds: None,
                    brave_api_key: None,
                    brave_endpoint: None,
                    include_arxiv: None,
                    include_pubmed: None,
                    include_uspto: None,
                }),
                firecrawl: Some(Firecrawl {
                    api_key: Some("fc-test".to_string()),
                    api_url: Some("https://firecrawl.example.com".to_string()),
                }),
                python: None,
            },
            &["default".to_string()],
            "",
        );

        let names: Vec<_> = agent
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();

        assert!(names.contains(&"subagent".to_string()));
        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"firecrawl".to_string()));
    }

    #[test]
    fn base_tools_are_registered_without_optional_web_tools() {
        let agent = BaseAgent::new(
            "default".to_string(),
            Arc::new(AgentConfig {
                model_registry: ModelRegistry::new(),
                max_iterations: 0,
                usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                ..Default::default()
            }),
            "System prompt".to_string(),
            Vec::new(),
        );

        configure_agent_tools(&agent, &Tools::default(), &["default".to_string()], "");

        let mut names: Vec<_> = agent
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();
        names.sort();

        assert_eq!(names, vec!["quiz".to_string(), "subagent".to_string()]);
    }

    #[test]
    fn configured_web_tools_are_registered_on_subagent() {
        let registry = subagent_tool_registry(&Tools {
            web_search: Some(WebSearch {
                searxng_endpoint: "https://search.example.com".to_string(),
                searxng_request_delay_seconds: None,
                brave_api_key: None,
                brave_endpoint: None,
                include_arxiv: None,
                include_pubmed: None,
                include_uspto: None,
            }),
            firecrawl: Some(Firecrawl {
                api_key: Some("fc-test".to_string()),
                api_url: Some("https://firecrawl.example.com".to_string()),
            }),
            python: None,
        });

        let names: Vec<_> = registry
            .definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();

        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"firecrawl".to_string()));
    }
}
