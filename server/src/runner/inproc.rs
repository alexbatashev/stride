use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, VecDeque},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use friday_agent::{
    AgentConfig, AgentResponseChunk, BaseAgent, Tool, ToolRegistry,
    tools::{
        arxiv::ArxivProvider,
        expert::{EXPERT_NAME, make_expert},
        firecrawl::FirecrawlTool,
        pubmed::PubmedProvider,
        web_search::{SearxngProvider, WebSearchTool},
    },
};
use futures::StreamExt;
use minisql::{ConnectionPool, Value};
use tokio::{
    runtime::Builder,
    sync::{broadcast, mpsc, oneshot, watch},
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
7. If you are using a source to extract a piece of information, always cite it properly. URLs for web pages, file names for files.
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
    Cancel {
        thread_id: Uuid,
        resp: oneshot::Sender<Result<(), AgentPoolError>>,
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
    event_history: VecDeque<AgentEvent>,
    cancel_tx: Option<watch::Sender<bool>>,
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
    tool_calls: BTreeMap<usize, PartialToolCall>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
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

    async fn cancel_run(&self, thread_id: Uuid) -> Result<(), AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.worker(thread_id)
            .tx
            .send(WorkerCommand::Cancel { thread_id, resp })
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
        WorkerCommand::Cancel { thread_id, resp } => {
            let result = handle_cancel(&state, thread_id);
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

    let cancel_rx = {
        let mut state = state.borrow_mut();
        let runner = state
            .threads
            .get_mut(&thread_id)
            .ok_or(AgentPoolError::ThreadNotFound)?;

        if !matches!(runner.status, ThreadStatus::Idle) {
            return Err(AgentPoolError::AlreadyRunning);
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        runner.cancel_tx = Some(cancel_tx);
        runner.status = ThreadStatus::Running { run_id };
        runner.last_used = Instant::now();
        cancel_rx
    };

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
        .tool_calls(Option::<&str>::None)
        .tool_call_id(Option::<&str>::None)
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

    tokio::task::spawn_local(run_agent_turn(
        state,
        thread_id,
        run_id,
        request.content,
        cancel_rx,
    ));

    Ok(run_id)
}

async fn handle_subscribe(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    after: Option<super::EventSeq>,
) -> Result<ThreadSubscription, AgentPoolError> {
    ensure_runner(state.clone(), thread_id).await?;

    let mut state = state.borrow_mut();
    let runner = state
        .threads
        .get_mut(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;

    runner.last_used = Instant::now();
    // Subscribe before reading last_event_seq so no events slip through the gap.
    let events = runner.event_tx.subscribe();
    let snapshot = ThreadSnapshot {
        thread_id,
        last_event_seq: runner.last_event_seq,
        status: runner.status.clone(),
        in_progress: runner.in_progress.clone(),
    };

    let replay = if let Some(after) = after {
        if after < runner.last_event_seq
            && runner
                .event_history
                .front()
                .is_none_or(|e| e.seq <= after + 1)
        {
            runner
                .event_history
                .iter()
                .filter(|e| e.seq > after)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(ThreadSubscription {
        snapshot,
        events,
        replay,
    })
}

fn handle_cancel(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid) -> Result<(), AgentPoolError> {
    let state = state.borrow();
    let runner = state
        .threads
        .get(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;
    if let Some(tx) = &runner.cancel_tx {
        let _ = tx.send(true);
    }
    Ok(())
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
            event_history: VecDeque::new(),
            cancel_tx: None,
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
    let mut providers: Vec<Box<dyn friday_agent::tools::web_search::SearchProvider>> =
        vec![Box::new(SearxngProvider {
            endpoint: web_search.searxng_endpoint.clone(),
        })];

    if web_search.include_arxiv == Some(true) {
        providers.push(Box::new(ArxivProvider));
    }

    if web_search.include_pubmed == Some(true) {
        providers.push(Box::new(PubmedProvider));
    }

    WebSearchTool {
        providers,
        ranker: Box::new(friday_agent::tools::web_search::InterleaveRanker),
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
    mut cancel_rx: watch::Receiver<bool>,
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
        tool_calls: BTreeMap::new(),
    };

    loop {
        tokio::select! {
            biased;
            _ = cancel_rx.changed() => {
                cancel_run_task(&state, thread_id, run_id);
                restore_agent(&state, thread_id, agent);
                return;
            }
            item = stream.next() => {
                let Some(item) = item else { break; };
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
                    Ok(AgentResponseChunk::ToolStarted { name, .. }) => {
                        with_runner(&state, thread_id, |runner| {
                            emit(
                                runner,
                                thread_id,
                                Some(run_id),
                                AgentEventKind::ToolStarted { name },
                            );
                        });
                    }
                    Ok(AgentResponseChunk::ToolFinished {
                        tool_call_id,
                        name,
                        result,
                    }) => {
                        if let Err(error) =
                            persist_tool_message(&state, thread_id, &tool_call_id, &result).await
                        {
                            fail_run(&state, thread_id, run_id, error.to_string());
                            restore_agent(&state, thread_id, agent);
                            return;
                        }

                        with_runner(&state, thread_id, |runner| {
                            emit(
                                runner,
                                thread_id,
                                Some(run_id),
                                AgentEventKind::ToolFinished { name },
                            );
                        });
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
        }
    }

    with_runner(&state, thread_id, |runner| {
        runner.cancel_tx = None;
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
    let mut has_message_delta = false;

    for choice in &chunk.choices {
        if let Some(message) = &choice.message {
            if !message.content.is_empty() {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                assistant.content.push_str(&message.content);
                with_runner(state, thread_id, |runner| {
                    emit(
                        runner,
                        thread_id,
                        Some(run_id),
                        AgentEventKind::AgentDelta {
                            content: message.content.clone(),
                        },
                    );
                });
            }

            if let Some(thinking) = message
                .thinking
                .as_ref()
                .filter(|thinking| !thinking.is_empty())
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
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

            if let Some(chunks) = message
                .tool_calls
                .as_ref()
                .filter(|chunks| has_tool_call_data(chunks))
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                append_tool_call_chunks(&mut assistant.tool_calls, chunks);
            }
        }

        if let Some(content) = choice.text.as_ref().filter(|content| !content.is_empty()) {
            ensure_assistant_message(state, thread_id, assistant).await?;
            has_message_delta = true;
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
            if let Some(content) = delta.content.as_ref().filter(|content| !content.is_empty()) {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
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

            if let Some(thinking) = delta
                .thinking
                .as_ref()
                .filter(|thinking| !thinking.is_empty())
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
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

            if let Some(chunks) = delta
                .tool_calls
                .as_ref()
                .filter(|chunks| has_tool_call_data(chunks))
            {
                ensure_assistant_message(state, thread_id, assistant).await?;
                has_message_delta = true;
                append_tool_call_chunks(&mut assistant.tool_calls, chunks);
            }
        }
    }

    if has_message_delta && let Some(id) = assistant.id {
        let db = state.borrow().db.clone();
        update_message(
            &db,
            id,
            &assistant.content,
            assistant.thinking.as_deref(),
            None,
        )
        .await?;

        with_runner(state, thread_id, |runner| {
            runner.in_progress = Some(PartialAgentMessage {
                run_id,
                content: assistant.content.clone(),
                thinking: assistant.thinking.clone(),
            });
        });
    }

    if chunk
        .choices
        .iter()
        .any(|choice| choice.finish_reason.is_some())
    {
        if let (Some(message_id), Some(seq)) = (assistant.id, assistant.seq) {
            let tool_calls = serialize_tool_calls(&assistant.tool_calls)?;
            let db = state.borrow().db.clone();
            update_message(
                &db,
                message_id,
                &assistant.content,
                assistant.thinking.as_deref(),
                tool_calls.as_deref(),
            )
            .await?;

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
        assistant.tool_calls.clear();
    }

    Ok(())
}

fn has_tool_call_data(chunks: &[llm::ToolCallChunk]) -> bool {
    chunks.iter().any(|chunk| {
        chunk.id.as_ref().is_some_and(|id| !id.is_empty())
            || chunk.function.as_ref().is_some_and(|function| {
                function.name.as_ref().is_some_and(|name| !name.is_empty())
                    || function
                        .arguments
                        .as_ref()
                        .is_some_and(|arguments| !arguments.is_empty())
            })
    })
}

fn append_tool_call_chunks(
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
    chunks: &[llm::ToolCallChunk],
) {
    for chunk in chunks {
        let index = chunk.index.unwrap_or(0);
        let call = tool_calls.entry(index).or_default();

        if let Some(id) = &chunk.id {
            call.id.push_str(id);
        }

        if let Some(function) = &chunk.function {
            if let Some(name) = &function.name {
                call.name.push_str(name);
            }
            if let Some(arguments) = &function.arguments {
                call.arguments.push_str(arguments);
            }
        }
    }
}

fn serialize_tool_calls(
    tool_calls: &BTreeMap<usize, PartialToolCall>,
) -> Result<Option<String>, AgentPoolError> {
    let calls: Vec<_> = tool_calls
        .values()
        .filter(|call| !call.name.is_empty())
        .map(|call| llm::ToolCallChunk {
            index: None,
            id: Some(call.id.clone()),
            function: Some(llm::ToolCallFunction {
                name: Some(call.name.clone()),
                arguments: Some(call.arguments.clone()),
            }),
        })
        .collect();

    if calls.is_empty() {
        return Ok(None);
    }

    serde_json::to_string(&calls)
        .map(Some)
        .map_err(|error| AgentPoolError::Internal(anyhow::anyhow!(error)))
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
        .tool_calls(Option::<&str>::None)
        .tool_call_id(Option::<&str>::None)
        .execute(&db)
        .await
        .map_err(db_error)?;

    assistant.id = Some(id);
    assistant.seq = Some(seq);
    assistant.content.clear();
    assistant.thinking = None;
    assistant.tool_calls.clear();

    Ok(())
}

async fn persist_tool_message(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    tool_call_id: &str,
    content: &str,
) -> Result<(), AgentPoolError> {
    let id = Uuid::now_v7();
    let seq = next_message_seq(state, thread_id)?;
    let db = state.borrow().db.clone();

    messages::insert()
        .id(id)
        .parent_thread(thread_id)
        .seq(seq)
        .role(Role::Tool)
        .content(content)
        .thinking(Option::<&str>::None)
        .tool_calls(Option::<&str>::None)
        .tool_call_id(Some(tool_call_id))
        .execute(&db)
        .await
        .map_err(db_error)?;

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
        runner.cancel_tx = None;
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

fn cancel_run_task(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid, run_id: RunId) {
    with_runner(state, thread_id, |runner| {
        runner.cancel_tx = None;
        runner.status = ThreadStatus::Idle;
        runner.in_progress = None;
        runner.last_used = Instant::now();
        emit(
            runner,
            thread_id,
            Some(run_id),
            AgentEventKind::RunCancelled,
        );
    });
}

fn emit(runner: &mut ThreadRunner, thread_id: Uuid, run_id: Option<RunId>, kind: AgentEventKind) {
    runner.last_event_seq += 1;
    let event = AgentEvent {
        seq: runner.last_event_seq,
        thread_id,
        run_id,
        kind,
    };
    runner.event_history.push_back(event.clone());
    if runner.event_history.len() > EVENT_BUFFER {
        runner.event_history.pop_front();
    }
    let _ = runner.event_tx.send(event);
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
            "SELECT seq, role, content, thinking, tool_calls, tool_call_id FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
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

async fn update_message(
    db: &ConnectionPool,
    id: Uuid,
    content: &str,
    thinking: Option<&str>,
    tool_calls: Option<&str>,
) -> Result<(), AgentPoolError> {
    db.query_with_params(
        "UPDATE messages SET content = ?, thinking = ?, tool_calls = ? WHERE id = ?",
        vec![
            Value::Text(content.to_string()),
            thinking
                .map(|s| Value::Text(s.to_string()))
                .unwrap_or(Value::Null),
            tool_calls
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
    use llm::{CompletionChoice, Delta, StreamResponseChunk, ToolCallChunk, ToolCallFunction};

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
                    include_arxiv: None,
                    include_pubmed: None,
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
                include_arxiv: None,
                include_pubmed: None,
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

    #[tokio::test]
    async fn send_persists_tool_calls_and_outputs() {
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
                    .with_stream_chunks(vec![
                        vec![tool_call_chunk("call-1", "missing_tool", r#"{"value":1}"#)],
                        vec![text_chunk("done")],
                    ])
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
        pool.send(
            thread_id,
            AgentRequest {
                content: "run tool".to_string(),
            },
        )
        .await
        .unwrap();

        let mut saw_tool_started = false;
        let mut saw_tool_finished = false;
        let mut saw_finished = false;
        for _ in 0..12 {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.events.recv())
                .await
                .unwrap()
                .unwrap();

            match event.kind {
                AgentEventKind::ToolStarted { name } if name == "missing_tool" => {
                    saw_tool_started = true;
                }
                AgentEventKind::ToolFinished { name } if name == "missing_tool" => {
                    saw_tool_finished = true;
                }
                AgentEventKind::RunFinished => {
                    saw_finished = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_tool_started);
        assert!(saw_tool_finished);
        assert!(saw_finished);

        let rows = db
            .query_with_params(
                "SELECT role, content, tool_calls, tool_call_id FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some(""));
        assert!(rows[1].get_text("tool_calls").is_some());
        assert_eq!(rows[2].get_text("role"), Some("tool"));
        assert_eq!(rows[2].get_text("tool_call_id"), Some("call-1"));
        assert!(
            rows[2]
                .get_text("content")
                .unwrap()
                .contains("unknown tool")
        );
        assert_eq!(rows[3].get_text("role"), Some("agent"));
        assert_eq!(rows[3].get_text("content"), Some("done"));

        let (thread, _) = load_thread(&db, thread_id).await.unwrap();
        assert_eq!(thread[1].tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(thread[2].tool_call_id.as_deref(), Some("call-1"));
    }

    #[tokio::test]
    async fn send_ignores_empty_stream_deltas() {
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
                    .with_stream_chunks(vec![vec![empty_delta_chunk(), text_chunk("pong")]])
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

        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
            },
        )
        .await
        .unwrap();

        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let rows = db
            .query_with_params(
                "SELECT role, content, thinking, tool_calls, tool_call_id FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get_text("role"), Some("user"));
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("pong"));
        assert!(matches!(rows[1].get("thinking"), Some(Value::Null) | None));
        assert!(matches!(
            rows[1].get("tool_calls"),
            Some(Value::Null) | None
        ));
        assert!(matches!(
            rows[1].get("tool_call_id"),
            Some(Value::Null) | None
        ));
    }

    #[tokio::test]
    async fn send_persists_full_choice_messages() {
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
                    .with_stream_chunks(vec![vec![message_chunk("think", "pong")]])
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

        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
            },
        )
        .await
        .unwrap();

        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let rows = db
            .query_with_params(
                "SELECT role, content, thinking FROM messages WHERE parent_thread = ? ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let rows = rows.rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].get_text("role"), Some("agent"));
        assert_eq!(rows[1].get_text("content"), Some("pong"));
        assert_eq!(rows[1].get_text("thinking"), Some("think"));
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

    fn empty_delta_chunk() -> StreamResponseChunk {
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
                    content: Some(String::new()),
                    thinking: Some(String::new()),
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: None,
            }],
        }
    }

    fn message_chunk(thinking: &str, content: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: Some(llm::Message {
                    role: llm::Role::Assistant,
                    content: content.to_string(),
                    thinking: Some(thinking.to_string()),
                    ..Default::default()
                }),
                text: None,
                index: 0,
                delta: None,
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("stop".to_string()),
            }],
        }
    }

    fn tool_call_chunk(id: &str, name: &str, arguments: &str) -> StreamResponseChunk {
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
                    content: None,
                    thinking: None,
                    tool_calls: Some(vec![ToolCallChunk {
                        index: Some(0),
                        id: Some(id.to_string()),
                        function: Some(ToolCallFunction {
                            name: Some(name.to_string()),
                            arguments: Some(arguments.to_string()),
                        }),
                    }]),
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("tool_calls".to_string()),
            }],
        }
    }

    #[tokio::test]
    async fn subscribe_with_after_replays_missed_events() {
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

        // Run to completion.
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
            },
        )
        .await
        .unwrap();
        while pool.status(thread_id).await.unwrap() != ThreadStatus::Idle {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Re-subscribe from seq=0 — should replay all events.
        let sub = pool.subscribe(thread_id, Some(0)).await.unwrap();
        assert!(
            !sub.replay.is_empty(),
            "expected replayed events after re-subscribe"
        );
        assert!(
            sub.replay
                .iter()
                .any(|e| matches!(e.kind, AgentEventKind::RunFinished)),
            "replay must include RunFinished"
        );

        // Re-subscribe from latest seq — replay should be empty.
        let sub2 = pool
            .subscribe(thread_id, Some(sub.snapshot.last_event_seq))
            .await
            .unwrap();
        assert!(
            sub2.replay.is_empty(),
            "no replay needed when already up to date"
        );
    }

    #[tokio::test]
    async fn cancel_run_terminates_cleanly() {
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

        let mut sub = pool.subscribe(thread_id, None).await.unwrap();
        pool.send(
            thread_id,
            AgentRequest {
                content: "ping".to_string(),
            },
        )
        .await
        .unwrap();
        pool.cancel_run(thread_id).await.unwrap();

        let mut terminated = false;
        for _ in 0..12 {
            let event = tokio::time::timeout(Duration::from_secs(2), sub.events.recv())
                .await
                .unwrap()
                .unwrap();
            match event.kind {
                AgentEventKind::RunCancelled | AgentEventKind::RunFinished => {
                    terminated = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(terminated, "run must terminate (cancelled or finished)");
        assert_eq!(pool.status(thread_id).await.unwrap(), ThreadStatus::Idle);
    }
}
