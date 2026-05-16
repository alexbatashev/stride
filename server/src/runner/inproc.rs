use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use friday_agent::{
    AgentConfig, AgentResponseChunk, BaseAgent, Tool, ToolRegistry,
    tools::{
        expert::{EXPERT_NAME, make_expert},
        firecrawl::FirecrawlTool,
        web_search::{SearxngProvider, WebSearchTool},
    },
};
use futures::StreamExt;
use minisql::{ConnectionPool, Value};
use tokio::{
    runtime::Builder,
    sync::{broadcast, mpsc, oneshot},
    task::LocalSet,
};
use uuid::Uuid;

use crate::{
    config::{Firecrawl, Tools, WebSearch},
    db::{Role, messages},
    runner::{
        AgentEvent, AgentEventKind, AgentPool, AgentPoolError, AgentRequest, PartialAgentMessage,
        RunId, ThreadSnapshot, ThreadStatus, ThreadSubscription,
    },
};

const WORKER_THREADS: usize = 8;
const EVENT_BUFFER: usize = 256;
const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);
const DEFAULT_SYSTEM_PROMPT: &str = "You are Friday, a semi-autonomous AI agent. Your task is to assist user with any requests.

Core instructions:

1. Use the tools available. Do not assume anything. If there's a tool that can solve the problem - use it.
2. You are running in a closed loop. Take time to achieve the goal. Call multiple tools if necessary.
3. Avoid ambiguity. If in doubt, clarify things with user BEFORE doing anything.
4. Think logically, step-by-step. During reasoning, use simplified language. Omit articles, use simple words, speak like a caveman.
5. Serve your human well. Abide by Asimov's tree laws of robotics. Do not be cruel or cowardly.
6. Use neutral wrting style unless asked otherwise. Avoid sounding like an AI or a robot, instead speak naturally. Do not use cliché.
";

pub struct InProcessAgentPool {
    workers: Vec<WorkerHandle>,
}

struct WorkerHandle {
    tx: mpsc::UnboundedSender<WorkerCommand>,
}

enum WorkerCommand {
    Send {
        thread_id: Uuid,
        request: AgentRequest,
        resp: oneshot::Sender<Result<RunId, AgentPoolError>>,
    },
    Subscribe {
        thread_id: Uuid,
        after: Option<super::EventSeq>,
        resp: oneshot::Sender<Result<ThreadSubscription, AgentPoolError>>,
    },
    Status {
        thread_id: Uuid,
        resp: oneshot::Sender<Result<ThreadStatus, AgentPoolError>>,
    },
    ShutdownThread {
        thread_id: Uuid,
        resp: oneshot::Sender<Result<(), AgentPoolError>>,
    },
}

struct WorkerState {
    db: ConnectionPool,
    config: Arc<AgentConfig>,
    tools: Tools,
    system_prompt: String,
    idle_ttl: Duration,
    threads: HashMap<Uuid, ThreadRunner>,
}

struct ThreadRunner {
    agent: Option<BaseAgent>,
    event_tx: broadcast::Sender<AgentEvent>,
    last_event_seq: u64,
    next_message_seq: u64,
    status: ThreadStatus,
    in_progress: Option<PartialAgentMessage>,
    last_used: Instant,
}

struct AssistantMessageState {
    id: Option<Uuid>,
    seq: Option<u64>,
    content: String,
    thinking: Option<String>,
}

impl InProcessAgentPool {
    pub fn new(db: ConnectionPool, config: Arc<AgentConfig>) -> Self {
        Self::with_system_prompt(db, config, DEFAULT_SYSTEM_PROMPT.to_string())
    }

    pub fn with_tool_config(db: ConnectionPool, config: Arc<AgentConfig>, tools: Tools) -> Self {
        Self::with_system_prompt_and_tools(db, config, DEFAULT_SYSTEM_PROMPT.to_string(), tools)
    }

    pub fn with_system_prompt(
        db: ConnectionPool,
        config: Arc<AgentConfig>,
        system_prompt: String,
    ) -> Self {
        Self::with_idle_ttl(db, config, system_prompt, DEFAULT_IDLE_TTL)
    }

    pub fn with_system_prompt_and_tools(
        db: ConnectionPool,
        config: Arc<AgentConfig>,
        system_prompt: String,
        tools: Tools,
    ) -> Self {
        Self::with_idle_ttl_and_tools(db, config, system_prompt, DEFAULT_IDLE_TTL, tools)
    }

    pub fn with_idle_ttl(
        db: ConnectionPool,
        config: Arc<AgentConfig>,
        system_prompt: String,
        idle_ttl: Duration,
    ) -> Self {
        Self::with_idle_ttl_and_tools(db, config, system_prompt, idle_ttl, Tools::default())
    }

    pub fn with_idle_ttl_and_tools(
        db: ConnectionPool,
        config: Arc<AgentConfig>,
        system_prompt: String,
        idle_ttl: Duration,
        tools: Tools,
    ) -> Self {
        let workers = (0..WORKER_THREADS)
            .map(|idx| {
                start_worker(
                    idx,
                    db.clone(),
                    config.clone(),
                    system_prompt.clone(),
                    idle_ttl,
                    tools.clone(),
                )
            })
            .collect();

        Self { workers }
    }

    fn worker(&self, thread_id: Uuid) -> &WorkerHandle {
        let idx = (thread_id.as_u128() as usize) % self.workers.len();
        &self.workers[idx]
    }
}

#[async_trait]
impl AgentPool for InProcessAgentPool {
    async fn send(&self, thread_id: Uuid, request: AgentRequest) -> Result<RunId, AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.worker(thread_id)
            .tx
            .send(WorkerCommand::Send {
                thread_id,
                request,
                resp,
            })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn subscribe(
        &self,
        thread_id: Uuid,
        after: Option<u64>,
    ) -> Result<ThreadSubscription, AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.worker(thread_id)
            .tx
            .send(WorkerCommand::Subscribe {
                thread_id,
                after,
                resp,
            })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn status(&self, thread_id: Uuid) -> Result<ThreadStatus, AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.worker(thread_id)
            .tx
            .send(WorkerCommand::Status { thread_id, resp })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn shutdown_thread(&self, thread_id: Uuid) -> Result<(), AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.worker(thread_id)
            .tx
            .send(WorkerCommand::ShutdownThread { thread_id, resp })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }
}

fn start_worker(
    idx: usize,
    db: ConnectionPool,
    config: Arc<AgentConfig>,
    system_prompt: String,
    idle_ttl: Duration,
    tools: Tools,
) -> WorkerHandle {
    let (tx, rx) = mpsc::unbounded_channel();

    std::thread::Builder::new()
        .name(format!("friday-agent-pool-{idx}"))
        .spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("agent worker runtime");
            let local = LocalSet::new();
            let state = Rc::new(RefCell::new(WorkerState {
                db,
                config,
                tools,
                system_prompt,
                idle_ttl,
                threads: HashMap::new(),
            }));

            local.block_on(&runtime, run_worker(state, rx));
        })
        .expect("agent worker thread");

    WorkerHandle { tx }
}

async fn run_worker(
    state: Rc<RefCell<WorkerState>>,
    mut rx: mpsc::UnboundedReceiver<WorkerCommand>,
) {
    let mut cleanup = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            command = rx.recv() => {
                let Some(command) = command else {
                    return;
                };
                handle_command(state.clone(), command).await;
            }
            _ = cleanup.tick() => {
                evict_idle_threads(&state);
            }
        }
    }
}

async fn handle_command(state: Rc<RefCell<WorkerState>>, command: WorkerCommand) {
    match command {
        WorkerCommand::Send {
            thread_id,
            request,
            resp,
        } => {
            let result = handle_send(state, thread_id, request).await;
            let _ = resp.send(result);
        }
        WorkerCommand::Subscribe {
            thread_id,
            after,
            resp,
        } => {
            let result = handle_subscribe(state, thread_id, after).await;
            let _ = resp.send(result);
        }
        WorkerCommand::Status { thread_id, resp } => {
            let result = handle_status(state, thread_id).await;
            let _ = resp.send(result);
        }
        WorkerCommand::ShutdownThread { thread_id, resp } => {
            state.borrow_mut().threads.remove(&thread_id);
            let _ = resp.send(Ok(()));
        }
    }
}

async fn handle_send(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    request: AgentRequest,
) -> Result<RunId, AgentPoolError> {
    ensure_runner(state.clone(), thread_id).await?;

    let run_id = RunId(Uuid::now_v7());

    {
        let mut state = state.borrow_mut();
        let runner = state
            .threads
            .get_mut(&thread_id)
            .ok_or(AgentPoolError::ThreadNotFound)?;

        if !matches!(runner.status, ThreadStatus::Idle) {
            return Err(AgentPoolError::AlreadyRunning);
        }

        runner.status = ThreadStatus::Running { run_id };
        runner.last_used = Instant::now();
    }

    let user_message_seq = next_message_seq(&state, thread_id)?;
    let user_message_id = Uuid::now_v7();
    let db = state.borrow().db.clone();

    if let Err(error) = messages::insert()
        .id(user_message_id)
        .parent_thread(thread_id)
        .seq(user_message_seq)
        .role(Role::User)
        .content(request.content.as_str())
        .thinking(Option::<&str>::None)
        .execute(&db)
        .await
        .map_err(db_error)
    {
        with_runner(&state, thread_id, |runner| {
            runner.status = ThreadStatus::Idle;
            runner.last_used = Instant::now();
        });
        return Err(error);
    }

    {
        let mut state = state.borrow_mut();
        let runner = state
            .threads
            .get_mut(&thread_id)
            .ok_or(AgentPoolError::ThreadNotFound)?;

        runner.last_used = Instant::now();
        emit(
            runner,
            thread_id,
            Some(run_id),
            AgentEventKind::UserMessageCommitted {
                message_id: user_message_id,
                seq: user_message_seq,
            },
        );
        emit(runner, thread_id, Some(run_id), AgentEventKind::RunStarted);
    }

    tokio::task::spawn_local(run_agent_turn(state, thread_id, run_id, request.content));

    Ok(run_id)
}

async fn handle_subscribe(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    _after: Option<super::EventSeq>,
) -> Result<ThreadSubscription, AgentPoolError> {
    ensure_runner(state.clone(), thread_id).await?;

    let mut state = state.borrow_mut();
    let runner = state
        .threads
        .get_mut(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;

    runner.last_used = Instant::now();
    let events = runner.event_tx.subscribe();
    let snapshot = ThreadSnapshot {
        thread_id,
        last_event_seq: runner.last_event_seq,
        status: runner.status.clone(),
        in_progress: runner.in_progress.clone(),
    };

    Ok(ThreadSubscription { snapshot, events })
}

async fn handle_status(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Result<ThreadStatus, AgentPoolError> {
    ensure_runner(state.clone(), thread_id).await?;

    let mut state = state.borrow_mut();
    let runner = state
        .threads
        .get_mut(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;
    runner.last_used = Instant::now();
    Ok(runner.status.clone())
}

async fn ensure_runner(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Result<(), AgentPoolError> {
    if state.borrow().threads.contains_key(&thread_id) {
        return Ok(());
    }

    let (db, config, tools, system_prompt) = {
        let state = state.borrow();
        (
            state.db.clone(),
            state.config.clone(),
            state.tools.clone(),
            state.system_prompt.clone(),
        )
    };

    ensure_thread_exists(&db, thread_id).await?;
    let (thread, next_message_seq) = load_thread(&db, thread_id).await?;
    let agent = BaseAgent::new("default".to_string(), config, system_prompt, thread);
    configure_agent_tools(&agent, &tools);
    let (event_tx, _) = broadcast::channel(EVENT_BUFFER);

    state.borrow_mut().threads.insert(
        thread_id,
        ThreadRunner {
            agent: Some(agent),
            event_tx,
            last_event_seq: 0,
            next_message_seq,
            status: ThreadStatus::Idle,
            in_progress: None,
            last_used: Instant::now(),
        },
    );

    Ok(())
}

fn configure_agent_tools(agent: &BaseAgent, tools: &Tools) {
    agent.register_tool(make_expert(expert_tool_registry(tools)));
    agent.allow_tool(EXPERT_NAME);

    if let Some(web_search) = &tools.web_search {
        agent.register_tool(web_search_tool(web_search));
    }

    if let Some(firecrawl) = &tools.firecrawl
        && let Some(tool) = firecrawl_tool(firecrawl)
    {
        agent.register_tool(tool);
    }
}

fn expert_tool_registry(tools: &Tools) -> ToolRegistry {
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

fn web_search_tool(web_search: &WebSearch) -> WebSearchTool {
    WebSearchTool {
        providers: vec![Box::new(SearxngProvider {
            endpoint: web_search.searxng_endpoint.clone(),
        })],
    }
}

fn firecrawl_tool(firecrawl: &Firecrawl) -> Option<FirecrawlTool> {
    firecrawl.read_api_key().map(|api_key| FirecrawlTool {
        api_key,
        api_url: firecrawl.api_url().to_string(),
    })
}

async fn run_agent_turn(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    content: String,
) {
    let agent = {
        let mut state = state.borrow_mut();
        let Some(runner) = state.threads.get_mut(&thread_id) else {
            return;
        };
        runner.agent.take()
    };

    let Some(agent) = agent else {
        fail_run(
            &state,
            thread_id,
            run_id,
            "agent is already running".to_string(),
        );
        return;
    };

    let mut stream = agent.make_turn(content).await;
    let mut assistant = AssistantMessageState {
        id: None,
        seq: None,
        content: String::new(),
        thinking: None,
    };

    while let Some(item) = stream.next().await {
        match item {
            Ok(AgentResponseChunk::Chunk(chunk)) => {
                if let Err(error) =
                    handle_agent_chunk(&state, thread_id, run_id, &mut assistant, chunk).await
                {
                    fail_run(&state, thread_id, run_id, error.to_string());
                    restore_agent(&state, thread_id, agent);
                    return;
                }
            }
            Ok(AgentResponseChunk::Approval {
                message, approved, ..
            }) => {
                let _ = approved.send(false);
                with_runner(&state, thread_id, |runner| {
                    emit(
                        runner,
                        thread_id,
                        Some(run_id),
                        AgentEventKind::WaitingForApproval {
                            approval_id: Uuid::now_v7(),
                            message,
                        },
                    );
                });
            }
            Ok(AgentResponseChunk::Quiz { answered, .. }) => {
                let _ = answered.send(vec![]);
            }
            Err(error) => {
                fail_run(&state, thread_id, run_id, error.to_string());
                restore_agent(&state, thread_id, agent);
                return;
            }
        }
    }

    with_runner(&state, thread_id, |runner| {
        runner.status = ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = Instant::now();
        emit(runner, thread_id, Some(run_id), AgentEventKind::RunFinished);
    });
    restore_agent(&state, thread_id, agent);
}

async fn handle_agent_chunk(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    assistant: &mut AssistantMessageState,
    chunk: llm::StreamResponseChunk,
) -> Result<(), AgentPoolError> {
    ensure_assistant_message(state, thread_id, assistant).await?;

    for choice in &chunk.choices {
        if let Some(content) = &choice.text {
            assistant.content.push_str(content);
            with_runner(state, thread_id, |runner| {
                emit(
                    runner,
                    thread_id,
                    Some(run_id),
                    AgentEventKind::AgentDelta {
                        content: content.clone(),
                    },
                );
            });
        }

        if let Some(delta) = &choice.delta {
            if let Some(content) = &delta.content {
                assistant.content.push_str(content);
                with_runner(state, thread_id, |runner| {
                    emit(
                        runner,
                        thread_id,
                        Some(run_id),
                        AgentEventKind::AgentDelta {
                            content: content.clone(),
                        },
                    );
                });
            }

            if let Some(thinking) = &delta.thinking {
                assistant
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(thinking);
                with_runner(state, thread_id, |runner| {
                    emit(
                        runner,
                        thread_id,
                        Some(run_id),
                        AgentEventKind::ThinkingDelta {
                            thinking: thinking.clone(),
                        },
                    );
                });
            }
        }
    }

    if let Some(id) = assistant.id {
        let db = state.borrow().db.clone();
        update_message(&db, id, &assistant.content, assistant.thinking.as_deref()).await?;
    }

    with_runner(state, thread_id, |runner| {
        runner.in_progress = Some(PartialAgentMessage {
            run_id,
            content: assistant.content.clone(),
            thinking: assistant.thinking.clone(),
        });
    });

    if chunk
        .choices
        .iter()
        .any(|choice| choice.finish_reason.is_some())
    {
        if let (Some(message_id), Some(seq)) = (assistant.id, assistant.seq) {
            with_runner(state, thread_id, |runner| {
                emit(
                    runner,
                    thread_id,
                    Some(run_id),
                    AgentEventKind::AgentMessageCommitted { message_id, seq },
                );
            });
        }

        assistant.id = None;
        assistant.seq = None;
        assistant.content.clear();
        assistant.thinking = None;
    }

    Ok(())
}

async fn ensure_assistant_message(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    assistant: &mut AssistantMessageState,
) -> Result<(), AgentPoolError> {
    if assistant.id.is_some() {
        return Ok(());
    }

    let id = Uuid::now_v7();
    let seq = next_message_seq(state, thread_id)?;
    let db = state.borrow().db.clone();

    messages::insert()
        .id(id)
        .parent_thread(thread_id)
        .seq(seq)
        .role(Role::Agent)
        .content("")
        .thinking(Option::<&str>::None)
        .execute(&db)
        .await
        .map_err(db_error)?;

    assistant.id = Some(id);
    assistant.seq = Some(seq);
    assistant.content.clear();
    assistant.thinking = None;

    Ok(())
}

fn next_message_seq(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Result<u64, AgentPoolError> {
    let mut state = state.borrow_mut();
    let runner = state
        .threads
        .get_mut(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;
    let seq = runner.next_message_seq;
    runner.next_message_seq += 1;
    Ok(seq)
}

fn with_runner(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    f: impl FnOnce(&mut ThreadRunner),
) {
    if let Some(runner) = state.borrow_mut().threads.get_mut(&thread_id) {
        f(runner);
    }
}

fn restore_agent(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, agent: BaseAgent) {
    with_runner(state, thread_id, |runner| {
        runner.agent = Some(agent);
        runner.last_used = Instant::now();
    });
}

fn fail_run(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, run_id: RunId, error: String) {
    with_runner(state, thread_id, |runner| {
        runner.status = ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = Instant::now();
        emit(
            runner,
            thread_id,
            Some(run_id),
            AgentEventKind::RunFailed { error },
        );
    });
}

fn emit(runner: &mut ThreadRunner, thread_id: Uuid, run_id: Option<RunId>, kind: AgentEventKind) {
    runner.last_event_seq += 1;
    let _ = runner.event_tx.send(AgentEvent {
        seq: runner.last_event_seq,
        thread_id,
        run_id,
        kind,
    });
}

fn evict_idle_threads(state: &Rc<RefCell<WorkerState>>) {
    let now = Instant::now();
    let mut state = state.borrow_mut();
    let idle_ttl = state.idle_ttl;

    state.threads.retain(|_, runner| {
        matches!(runner.status, ThreadStatus::Running { .. })
            || now.duration_since(runner.last_used) < idle_ttl
    });
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

async fn load_thread(
    db: &ConnectionPool,
    thread_id: Uuid,
) -> Result<(Vec<llm::Message>, u64), AgentPoolError> {
    let result = db
        .query_with_params(
            "SELECT seq, role, content, thinking FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
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
            thinking: match row.get("thinking") {
                Some(Value::Text(thinking)) => Some(thinking.clone()),
                _ => None,
            },
            ..Default::default()
        });

        next_seq = seq + 1;
    }

    Ok((thread, next_seq))
}

async fn update_message(
    db: &ConnectionPool,
    id: Uuid,
    content: &str,
    thinking: Option<&str>,
) -> Result<(), AgentPoolError> {
    db.query_with_params(
        "UPDATE messages SET content = ?, thinking = ? WHERE id = ?",
        vec![
            Value::Text(content.to_string()),
            thinking
                .map(|s| Value::Text(s.to_string()))
                .unwrap_or(Value::Null),
            Value::Uuid(id),
        ],
    )
    .await
    .map_err(db_error)?;

    Ok(())
}

fn db_error(err: Box<dyn std::error::Error + Send + Sync>) -> AgentPoolError {
    AgentPoolError::Internal(anyhow::anyhow!(err.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::config::{Firecrawl, WebSearch};
    use friday_agent::{AgentConfig, DEFAULT_MODEL, ModelRegEntry, ModelRegistry};
    use llm::{CompletionChoice, Delta, StreamResponseChunk};

    use super::*;
    use crate::db::{self, threads, users};

    #[test]
    fn configured_tools_are_registered_on_agent() {
        let agent = BaseAgent::new(
            "default".to_string(),
            Arc::new(AgentConfig {
                model_registry: ModelRegistry::new(),
                max_iterations: 0,
            }),
            "System prompt".to_string(),
            Vec::new(),
        );

        configure_agent_tools(
            &agent,
            &Tools {
                web_search: Some(WebSearch {
                    searxng_endpoint: "https://search.example.com".to_string(),
                }),
                firecrawl: Some(Firecrawl {
                    api_key: Some("fc-test".to_string()),
                    api_url: Some("https://firecrawl.example.com".to_string()),
                }),
            },
        );

        let names: Vec<_> = agent
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();

        assert!(names.contains(&"expert".to_string()));
        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"firecrawl".to_string()));
    }

    #[test]
    fn expert_is_registered_without_optional_web_tools() {
        let agent = BaseAgent::new(
            "default".to_string(),
            Arc::new(AgentConfig {
                model_registry: ModelRegistry::new(),
                max_iterations: 0,
            }),
            "System prompt".to_string(),
            Vec::new(),
        );

        configure_agent_tools(&agent, &Tools::default());

        let names: Vec<_> = agent
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();

        assert_eq!(names, vec!["expert".to_string()]);
    }

    #[test]
    fn configured_web_tools_are_registered_on_expert() {
        let registry = expert_tool_registry(&Tools {
            web_search: Some(WebSearch {
                searxng_endpoint: "https://search.example.com".to_string(),
            }),
            firecrawl: Some(Firecrawl {
                api_key: Some("fc-test".to_string()),
                api_url: Some("https://firecrawl.example.com".to_string()),
            }),
        });

        let names: Vec<_> = registry
            .definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();

        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"firecrawl".to_string()));
    }

    #[tokio::test]
    async fn send_persists_messages_and_streams_events() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();

        users::insert()
            .id(owner)
            .username("alice")
            .password_hash("hash")
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title("Test")
            .execute(&db)
            .await
            .unwrap();

        let mut models = ModelRegistry::new();
        models.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![vec![text_chunk("pong")]])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                thinking: false,
            },
        );

        let pool = InProcessAgentPool::with_idle_ttl(
            db.clone(),
            Arc::new(AgentConfig {
                model_registry: models,
                max_iterations: 4,
            }),
            "System prompt".to_string(),
            Duration::from_secs(60),
        );

        let mut subscription = pool.subscribe(thread_id, None).await.unwrap();
        let run_id = pool
            .send(
                thread_id,
                AgentRequest {
                    content: "ping".to_string(),
                },
            )
            .await
            .unwrap();

        let mut saw_delta = false;
        let mut saw_finished = false;
        for _ in 0..8 {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.events.recv())
                .await
                .unwrap()
                .unwrap();

            assert_eq!(event.thread_id, thread_id);
            assert_eq!(event.run_id, Some(run_id));

            match event.kind {
                AgentEventKind::AgentDelta { content } if content == "pong" => {
                    saw_delta = true;
                }
                AgentEventKind::RunFinished => {
                    saw_finished = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_delta);
        assert!(saw_finished);
        assert_eq!(pool.status(thread_id).await.unwrap(), ThreadStatus::Idle);

        let rows = db
            .query_with_params(
                "SELECT role, content FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get_text("role"), Some("user"));
        assert_eq!(rows[0].get_text("content"), Some("ping"));
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("pong"));
    }

    fn text_chunk(content: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: None,
                text: None,
                index: 0,
                delta: Some(Delta {
                    content: Some(content.to_string()),
                    thinking: None,
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("stop".to_string()),
            }],
        }
    }
}
