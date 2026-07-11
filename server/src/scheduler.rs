//! Automation executor. Runs on a dedicated current-thread runtime because
//! agent and python execution futures are `!Send`. It owns a fire-request
//! channel that unifies every trigger source: the cron/vfs poll loop produces
//! requests internally, and webhook/manual API handlers produce them through an
//! [`ExecutorHandle`]. Each request runs the same [`run_automation`] pipeline.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures::StreamExt;
use minisql::ConnectionPool;
use serde_json::Value as JsonValue;
use stride_agent::{
    AgentConfig, AutoDenyInteractionBroker, BaseAgent, EventKind, NoopEventSink, Tool, TurnContext,
    mcp::McpTool,
    tools::email::{CreateEmailDraftTool, ListEmailsTool},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use uuid::Uuid;

use crate::config::Tools;
use crate::db::{AutomationKind, NotifyKind, RunStatus, TriggerKind, automation_runs, automations};
use crate::email::ImapService;
use crate::google::GoogleService;
use crate::notify::{self, RunResult};
use crate::runner::bootstrap::{
    ScriptableToolRegistryContext, python_tool_config, scriptable_tool_registry,
};
use crate::triggers;

const POLL_SECS: u64 = 60;
const AGENT_SYSTEM_PROMPT: &str = "You are Stride, running a scheduled automation with no interactive user. \
     Complete the task and produce a concise final report. You cannot ask questions.";

/// What caused a fire request, kept for logging and run metadata.
#[derive(Clone, Copy, Debug)]
pub enum TriggerSource {
    Cron,
    Webhook,
    Manual,
    VfsChange,
    Email,
    Gmail,
}

/// A request to run one automation now, optionally with a trigger payload.
pub struct FireRequest {
    pub automation_id: Uuid,
    pub payload: Option<JsonValue>,
    pub source: TriggerSource,
}

#[derive(Debug)]
pub enum ExecutorError {
    Closed,
}

/// Cloneable handle for producing fire requests from other threads (Axum
/// handlers). `send` is non-blocking and `Send`, so it never crosses the
/// executor's `!Send` boundary.
#[derive(Clone)]
pub struct ExecutorHandle {
    tx: UnboundedSender<FireRequest>,
}

impl ExecutorHandle {
    pub fn fire(&self, req: FireRequest) -> Result<(), ExecutorError> {
        self.tx.send(req).map_err(|_| ExecutorError::Closed)
    }

    /// Build a handle plus its receiver without spawning the executor thread.
    /// Useful for tests and for wiring the channel manually.
    #[cfg(test)]
    pub fn channel() -> (ExecutorHandle, UnboundedReceiver<FireRequest>) {
        let (tx, rx) = unbounded_channel();
        (ExecutorHandle { tx }, rx)
    }
}

/// Shared dependencies every run needs.
#[derive(Clone)]
pub struct ExecutorConfig {
    pub db: ConnectionPool,
    pub model_config: Arc<AgentConfig>,
    pub tools: Tools,
    pub searchable_tools_preview_limit: usize,
    pub telegram_bot_token: Option<String>,
    pub email_service: ImapService,
    pub mcp_tools: Vec<McpTool>,
    pub google_service: Option<GoogleService>,
}

pub fn spawn(config: ExecutorConfig) -> ExecutorHandle {
    let (tx, rx) = unbounded_channel();
    std::thread::Builder::new()
        .name("stride-scheduler".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("scheduler runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&runtime, run(config, rx));
        })
        .expect("scheduler thread");
    ExecutorHandle { tx }
}

async fn run(ctx: ExecutorConfig, mut rx: UnboundedReceiver<FireRequest>) {
    let mut tick = tokio::time::interval(Duration::from_secs(POLL_SECS));
    loop {
        tokio::select! {
            _ = tick.tick() => poll_due(&ctx).await,
            request = rx.recv() => match request {
                Some(request) => {
                    let ctx = ctx.clone();
                    tokio::task::spawn_local(async move { run_automation(ctx, request).await });
                }
                None => break,
            },
        }
    }
}

/// Evaluate polled triggers (cron, vfs change) and fire the due ones.
async fn poll_due(ctx: &ExecutorConfig) {
    let now = ctx.model_config.clock.now_unix_secs();
    let rows = match automations::select()
        .where_(automations::enabled.eq(true))
        .all(&ctx.db)
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "scheduler failed to load automations");
            return;
        }
    };

    for row in rows {
        let kind = TriggerKind::from_opt(row.trigger_kind.as_deref());
        if kind == TriggerKind::Email {
            let ctx = ctx.clone();
            tokio::task::spawn_local(async move { poll_email(&ctx, row).await });
            continue;
        }
        if kind == TriggerKind::Gmail {
            let ctx = ctx.clone();
            tokio::task::spawn_local(async move { poll_gmail(&ctx, row).await });
            continue;
        }
        let Some(trigger) = triggers::polled(
            kind,
            &row.schedule,
            row.trigger_config.as_deref(),
            row.owner,
        ) else {
            continue;
        };
        if !trigger.due(&ctx.db, now, row.last_run).await {
            continue;
        }
        // Mark before dispatch so a slow run is not retriggered next tick, and so
        // the vfs watermark advances.
        if let Err(error) = automations::update()
            .last_run(Some(now))
            .where_(automations::id.eq(row.id))
            .execute(&ctx.db)
            .await
        {
            tracing::warn!(%error, "scheduler failed to mark automation");
            continue;
        }
        let ctx = ctx.clone();
        let source = match kind {
            TriggerKind::VfsChange => TriggerSource::VfsChange,
            TriggerKind::Email => TriggerSource::Email,
            _ => TriggerSource::Cron,
        };
        tokio::task::spawn_local(async move {
            run_automation(
                ctx,
                FireRequest {
                    automation_id: row.id,
                    payload: None,
                    source,
                },
            )
            .await
        });
    }
}

async fn poll_email(ctx: &ExecutorConfig, automation: crate::db::automations::Row) {
    let account_id = automation
        .trigger_config
        .as_deref()
        .and_then(|config| serde_json::from_str::<JsonValue>(config).ok())
        .and_then(|config| config.get("account_id")?.as_str().map(str::to_string))
        .and_then(|id| Uuid::parse_str(&id).ok());
    let Some(account_id) = account_id else {
        tracing::warn!(automation_id = %automation.id, "email trigger has invalid account config");
        return;
    };
    let cursor = automation
        .trigger_cursor
        .as_deref()
        .and_then(|cursor| cursor.parse::<u32>().ok())
        .unwrap_or(0);
    let batch = match ctx
        .email_service
        .new_inbox_messages(automation.owner, account_id, cursor)
        .await
    {
        Ok(batch) => batch,
        Err(error) => {
            tracing::warn!(%error, automation_id = %automation.id, "email trigger poll failed");
            return;
        }
    };
    if batch.messages.is_empty() {
        return;
    }
    if let Err(error) = automations::update()
        .last_run(Some(ctx.model_config.clock.now_unix_secs()))
        .trigger_cursor(Some(batch.cursor.to_string().as_str()))
        .where_(automations::id.eq(automation.id))
        .execute(&ctx.db)
        .await
    {
        tracing::warn!(%error, "scheduler failed to advance email cursor");
        return;
    }
    let payload = serde_json::json!({
        "account_id": account_id,
        "mailbox": "inbox",
        "messages": batch.messages,
    });
    let ctx = ctx.clone();
    tokio::task::spawn_local(async move {
        run_automation(
            ctx,
            FireRequest {
                automation_id: automation.id,
                payload: Some(payload),
                source: TriggerSource::Email,
            },
        )
        .await
    });
}

/// Poll a Gmail-triggered automation: fetch inbox messages newer than the stored
/// watermark and fire the automation with them as the payload.
async fn poll_gmail(ctx: &ExecutorConfig, automation: crate::db::automations::Row) {
    let Some(service) = ctx.google_service.as_ref() else {
        return;
    };
    let cursor = automation
        .trigger_cursor
        .as_deref()
        .and_then(|cursor| cursor.parse::<i64>().ok())
        .unwrap_or(0);
    let batch = match service.gmail_new_since(automation.owner, cursor).await {
        Ok(batch) => batch,
        Err(error) => {
            tracing::warn!(%error, automation_id = %automation.id, "gmail trigger poll failed");
            return;
        }
    };
    if batch.messages.is_empty() {
        return;
    }
    if let Err(error) = automations::update()
        .last_run(Some(ctx.model_config.clock.now_unix_secs()))
        .trigger_cursor(Some(batch.cursor.to_string().as_str()))
        .where_(automations::id.eq(automation.id))
        .execute(&ctx.db)
        .await
    {
        tracing::warn!(%error, "scheduler failed to advance gmail cursor");
        return;
    }
    let payload = serde_json::json!({
        "source": "gmail",
        "messages": batch.messages,
    });
    let ctx = ctx.clone();
    tokio::task::spawn_local(async move {
        run_automation(
            ctx,
            FireRequest {
                automation_id: automation.id,
                payload: Some(payload),
                source: TriggerSource::Gmail,
            },
        )
        .await
    });
}

async fn run_automation(ctx: ExecutorConfig, request: FireRequest) {
    let rows = match automations::select()
        .where_(automations::id.eq(request.automation_id))
        .all(&ctx.db)
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "scheduler failed to load automation");
            return;
        }
    };
    let Some(automation) = rows.into_iter().next() else {
        tracing::warn!(automation_id = %request.automation_id, "automation not found");
        return;
    };
    if !automation.enabled {
        return;
    }

    let run_id = ctx.model_config.id_gen.new_uuid_v7();
    let started_at = ctx.model_config.clock.now_unix_secs();
    if let Err(error) = automation_runs::insert()
        .id(run_id)
        .automation_id(automation.id)
        .started_at(started_at)
        .status(RunStatus::Running)
        .output("")
        .execute(&ctx.db)
        .await
    {
        tracing::warn!(%error, "scheduler failed to record run start");
        return;
    }

    tracing::info!(
        automation = %automation.name,
        source = ?request.source,
        "running automation"
    );

    // Offer the native Google tools to the run only when the owner is linked.
    let google = match ctx.google_service.as_ref() {
        Some(service) if service.is_connected(automation.owner).await => {
            Some((service.clone(), automation.owner))
        }
        _ => None,
    };

    let result = match automation.kind {
        AutomationKind::Python => {
            let script = python_script(&automation.payload, request.payload.as_ref());
            let mut mcp_tools = ctx.mcp_tools.clone();
            mcp_tools.extend(
                crate::mcp_servers::connect_user_mcp_servers(&ctx.db, automation.owner).await,
            );
            run_python(&ctx, automation.owner, &script, mcp_tools, google).await
        }
        AutomationKind::Agent => {
            let prompt = agent_prompt(&automation.payload, request.payload.as_ref());
            let mut mcp_tools = ctx.mcp_tools.clone();
            mcp_tools.extend(
                crate::mcp_servers::connect_user_mcp_servers(&ctx.db, automation.owner).await,
            );
            run_agent(
                ctx.model_config.clone(),
                &prompt,
                ctx.email_service.provider(automation.owner),
                mcp_tools,
                google,
                ctx.searchable_tools_preview_limit,
            )
            .await
        }
    };
    let (status, output) = match result {
        Ok(output) => (RunStatus::Success, output),
        Err(output) => (RunStatus::Failed, output),
    };

    if let Err(error) = automation_runs::update()
        .finished_at(Some(ctx.model_config.clock.now_unix_secs()))
        .status(status)
        .output(output.as_str())
        .where_(automation_runs::id.eq(run_id))
        .execute(&ctx.db)
        .await
    {
        tracing::warn!(%error, "scheduler failed to record run result");
    }

    // Conversation is stored above; notify is additive and best-effort.
    let notify_kind = NotifyKind::from_opt(automation.notify_kind.as_deref());
    let notifier = notify::build(
        notify_kind,
        automation.owner,
        &ctx.db,
        ctx.telegram_bot_token.as_deref(),
    );
    let run_result = RunResult {
        name: &automation.name,
        status,
        output: &output,
    };
    if let Err(error) = notifier.notify(&run_result).await {
        tracing::warn!(%error, "automation notification failed");
    }
}

/// Append the trigger payload to the agent prompt as a fenced data block.
fn agent_prompt(base: &str, payload: Option<&JsonValue>) -> String {
    match payload {
        Some(value) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
            format!("{base}\n\n---\nTrigger payload (JSON):\n```json\n{pretty}\n```")
        }
        None => base.to_string(),
    }
}

/// Expose the trigger payload to the python script as a `PAYLOAD` dict. The JSON
/// is base64-encoded to sidestep any string-escaping hazards.
fn python_script(base: &str, payload: Option<&JsonValue>) -> String {
    match payload {
        Some(value) => {
            let json = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
            let encoded = BASE64.encode(json.as_bytes());
            format!(
                "import json as _json, base64 as _b64\n\
                 PAYLOAD = _json.loads(_b64.b64decode(\"{encoded}\").decode())\n\
                 {base}"
            )
        }
        None => base.to_string(),
    }
}

async fn run_python(
    ctx: &ExecutorConfig,
    owner: Uuid,
    script: &str,
    mcp_tools: Vec<McpTool>,
    google: Option<(GoogleService, Uuid)>,
) -> Result<String, String> {
    let Some(python) = ctx
        .tools
        .python
        .as_ref()
        .filter(|p| p.enabled != Some(false))
    else {
        return Err("python execution is disabled".to_string());
    };
    let config = python_tool_config(python);
    let dir = config.cache_dir.join("automations").join(
        ctx.model_config
            .id_gen
            .new_uuid_v7()
            .as_simple()
            .to_string(),
    );
    let fs = execenv::DirectOsFileSystem::new(dir).map_err(|e| e.to_string())?;
    // Same tool surface as the interactive agent loop, so scripts that work
    // there work here too.
    let registry = scriptable_tool_registry(ScriptableToolRegistryContext {
        tools: &ctx.tools,
        db: &ctx.db,
        user_id: owner,
        email_provider: Some(ctx.email_service.provider(owner)),
        mcp_tools: &mcp_tools,
        default_wing: None,
        google,
    });
    let tool = execenv::PythonTool::new(config, Arc::new(fs))
        .await
        .map_err(|e| e.to_string())?
        .with_tools(registry);

    let result = tool
        .execute(
            ctx.model_config.clone(),
            serde_json::json!({ "script": script }),
        )
        .await;
    let stdout = result["stdout"].as_str().unwrap_or_default();
    let stderr = result["stderr"].as_str().unwrap_or_default();
    let output = format!("{stdout}{stderr}");
    if result["success"].as_bool().unwrap_or(false) {
        Ok(output)
    } else {
        let error = result["error"]
            .as_str()
            .unwrap_or("python execution failed");
        Err(format!("{output}{error}"))
    }
}

async fn run_agent(
    model_config: Arc<AgentConfig>,
    prompt: &str,
    email_provider: Arc<dyn stride_agent::tools::email::EmailProvider>,
    mcp_tools: Vec<McpTool>,
    google: Option<(GoogleService, Uuid)>,
    searchable_tools_preview_limit: usize,
) -> Result<String, String> {
    let run_id = model_config.id_gen.new_uuid_v7();
    let agent = BaseAgent::new(
        "default".to_string(),
        model_config,
        AGENT_SYSTEM_PROMPT.to_string(),
        Vec::new(),
    );
    agent.set_searchable_tools_preview_limit(searchable_tools_preview_limit);
    for tool in mcp_tools {
        agent.register_searchable_tool(tool);
    }
    agent.register_tool(ListEmailsTool {
        provider: email_provider.clone(),
    });
    agent.allow_tool("list_emails");
    agent.register_tool(CreateEmailDraftTool {
        provider: email_provider,
    });
    agent.allow_tool("create_email_draft");
    if let Some((service, user)) = google {
        crate::tools::google::register(&agent, service, user);
    }
    let context = TurnContext::new(
        run_id,
        Arc::new(NoopEventSink),
        Arc::new(AutoDenyInteractionBroker),
    );
    let mut stream = agent
        .make_turn(prompt.to_string(), Vec::new(), context)
        .await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event.kind {
            EventKind::TextDelta { delta, .. } => output.push_str(&delta),
            EventKind::ToolCallFinished { name, result, .. } => {
                if !result.trim().is_empty() {
                    if !output.is_empty() {
                        output.push_str("\n\n");
                    }
                    output.push_str(&format!("{name} output:\n{result}"));
                }
            }
            EventKind::RunFailed { error } => return Err(format!("{output}\n{error}")),
            _ => {}
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, AutomationKind, RunStatus, automation_runs, automations, users};

    async fn seed_user(db: &ConnectionPool) -> Uuid {
        let owner = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username(format!("u{}", owner.as_simple()).as_str())
            .password_hash("x")
            .execute(db)
            .await
            .unwrap();
        owner
    }

    fn python_tools() -> Tools {
        Tools {
            python: Some(crate::config::Python {
                enabled: Some(true),
                cache_dir: None,
                backend: Some(crate::config::PythonBackend::Mock),
                threads: Some(1),
                preinit: Some(false),
                max_runtime_seconds: None,
                max_memory_bytes: None,
                max_cpu_fuel: None,
                network: None,
            }),
            ..Default::default()
        }
    }

    fn mock_model_config() -> Arc<AgentConfig> {
        Arc::new(AgentConfig {
            model_registry: stride_agent::ModelRegistry::new(),
            max_iterations: 2,
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            ..Default::default()
        })
    }

    #[test]
    fn agent_prompt_injects_payload() {
        let prompt = agent_prompt("do it", Some(&serde_json::json!({"a": 1})));
        assert!(prompt.contains("do it"));
        assert!(prompt.contains("Trigger payload"));
        assert!(prompt.contains("\"a\""));
        assert_eq!(agent_prompt("plain", None), "plain");
    }

    #[test]
    fn python_script_injects_payload() {
        let script = python_script("print(PAYLOAD)", Some(&serde_json::json!({"k": "v"})));
        assert!(script.contains("PAYLOAD = _json.loads"));
        assert!(script.ends_with("print(PAYLOAD)"));
        assert_eq!(python_script("noop", None), "noop");
    }

    #[tokio::test]
    async fn run_automation_executes_python_and_records_run() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let db = ConnectionPool::new("sqlite::memory:").unwrap();
                db.initialize_database(db::get_migrations()).await.unwrap();
                let owner = seed_user(&db).await;

                let id = Uuid::now_v7();
                automations::insert()
                    .id(id)
                    .owner(owner)
                    .name("echo")
                    .schedule("")
                    .kind(AutomationKind::Python)
                    .payload("print('hello')")
                    .enabled(true)
                    .created_at(1)
                    .trigger_kind(Some("webhook"))
                    .notify_kind(Some("none"))
                    .execute(&db)
                    .await
                    .unwrap();

                let ctx = ExecutorConfig {
                    db: db.clone(),
                    model_config: mock_model_config(),
                    tools: python_tools(),
                    searchable_tools_preview_limit: 20,
                    telegram_bot_token: None,
                    email_service: ImapService::with_clock(
                        db.clone(),
                        "test-secret",
                        std::sync::Arc::new(stride_agent::SystemClock),
                        std::sync::Arc::new(stride_agent::SystemIdGen),
                    ),
                    mcp_tools: Vec::new(),
                    google_service: None,
                };
                run_automation(
                    ctx,
                    FireRequest {
                        automation_id: id,
                        payload: Some(serde_json::json!({"n": 7})),
                        source: TriggerSource::Webhook,
                    },
                )
                .await;

                let runs = automation_runs::select()
                    .where_(automation_runs::automation_id.eq(id))
                    .all(&db)
                    .await
                    .unwrap();
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].status, RunStatus::Success);
                assert!(runs[0].finished_at.is_some());
            })
            .await;
    }
}
