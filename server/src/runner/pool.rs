use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::channel::oneshot as futures_oneshot;
use minisql::ConnectionPool;
use stride_agent::{AgentConfig, BaseAgent, QuizQuestion, mcp::McpTool};
use tokio::{
    runtime::Builder,
    sync::{mpsc, oneshot, watch},
    task::LocalSet,
};
use uuid::Uuid;

use crate::{
    config,
    crypto::SecretCipher,
    db::{MessageFormat, Role, messages, threads},
    email::ImapService,
    github::GitHubRuntime,
    google::GoogleService,
    runner::{
        AgentEventKind, AgentPool, AgentPoolError, AgentRequest, PartialAgentMessage,
        PendingApproval, PendingQuiz, RUNNER_LIFECYCLE_TOPIC, RunId, RunnerLifecycle,
        ThreadSnapshot, ThreadStatus, db_error, thread_events_topic,
    },
    vfs::Vfs,
};

use super::bootstrap::ensure_runner;
use super::inproc::{emit, run_agent_turn};
use super::prompt::BASE_SYSTEM_PROMPT;

const WORKER_THREADS: usize = 8;
const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);

pub struct InProcessAgentPool {
    pool: PoolHandle,
}

/// Routing table over every worker's command channel. Cloned into each worker so
/// tools running inside a turn (such as `start_thread`) can enqueue a run on a
/// freshly created thread through the same path the public API uses.
#[derive(Clone)]
pub(crate) struct PoolHandle {
    senders: Arc<Vec<mpsc::UnboundedSender<WorkerCommand>>>,
}

impl PoolHandle {
    fn worker(&self, thread_id: Uuid) -> &mpsc::UnboundedSender<WorkerCommand> {
        let idx = (thread_id.as_u128() as usize) % self.senders.len();
        &self.senders[idx]
    }

    /// A handle backed by a single dead channel, for tests that build a
    /// `WorkerState` directly and never route through the pool.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        PoolHandle {
            senders: Arc::new(vec![tx]),
        }
    }

    /// Deliver `request` to a thread's worker, starting its next run.
    pub(crate) async fn send(
        &self,
        thread_id: Uuid,
        request: AgentRequest,
    ) -> Result<RunId, AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.worker(thread_id)
            .send(WorkerCommand::Send {
                thread_id,
                request,
                resp,
            })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }
}

pub(crate) enum WorkerCommand {
    Send {
        thread_id: Uuid,
        request: AgentRequest,
        resp: oneshot::Sender<Result<RunId, AgentPoolError>>,
    },
    Snapshot {
        thread_id: Uuid,
        resp: oneshot::Sender<Result<ThreadSnapshot, AgentPoolError>>,
    },
    Cancel {
        thread_id: Uuid,
        resp: oneshot::Sender<Result<(), AgentPoolError>>,
    },
    ResolveApproval {
        thread_id: Uuid,
        approval_id: Uuid,
        approved: bool,
        resp: oneshot::Sender<Result<(), AgentPoolError>>,
    },
    AnswerQuiz {
        thread_id: Uuid,
        quiz_id: Uuid,
        answers: Vec<String>,
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

#[derive(Clone)]
pub(crate) struct WorkerInit {
    pub(crate) db: ConnectionPool,
    pub(crate) config: Arc<AgentConfig>,
    pub(crate) server_config: config::Config,
    pub(crate) cipher: SecretCipher,
    pub(crate) tools: config::Tools,
    pub(crate) mcp_tools: Vec<McpTool>,
    pub(crate) vfs: Option<Arc<Vfs>>,
    pub(crate) telegram_bot_token: Option<String>,
    pub(crate) public_url: Option<String>,
    pub(crate) github_runtime: Option<GitHubRuntime>,
    pub(crate) email_service: Option<ImapService>,
    pub(crate) google_service: Option<GoogleService>,
    pub(crate) system_prompt: String,
    pub(crate) idle_ttl: Duration,
}

pub(crate) struct InProcessAgentPoolBuilder {
    init: WorkerInit,
}

impl InProcessAgentPoolBuilder {
    pub(crate) fn tools(mut self, tools: config::Tools) -> Self {
        self.init.tools = tools;
        self
    }

    pub(crate) fn mcp_tools(mut self, mcp_tools: Vec<McpTool>) -> Self {
        self.init.mcp_tools = mcp_tools;
        self
    }

    pub(crate) fn vfs(mut self, vfs: Arc<Vfs>) -> Self {
        self.init.vfs = Some(vfs);
        self
    }

    pub(crate) fn telegram_bot_token(mut self, telegram_bot_token: Option<String>) -> Self {
        self.init.telegram_bot_token = telegram_bot_token;
        self
    }

    pub(crate) fn public_url(mut self, public_url: Option<String>) -> Self {
        self.init.public_url = public_url;
        self
    }

    pub(crate) fn github_runtime(mut self, github_runtime: Option<GitHubRuntime>) -> Self {
        self.init.github_runtime = github_runtime;
        self
    }

    pub(crate) fn email_service(mut self, email_service: ImapService) -> Self {
        self.init.email_service = Some(email_service);
        self
    }

    pub(crate) fn google_service(mut self, google_service: Option<GoogleService>) -> Self {
        self.init.google_service = google_service;
        self
    }

    pub(crate) fn system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
        self.init.system_prompt = system_prompt.into();
        self
    }

    pub(crate) fn idle_ttl(mut self, idle_ttl: Duration) -> Self {
        self.init.idle_ttl = idle_ttl;
        self
    }

    pub(crate) fn build(self) -> InProcessAgentPool {
        InProcessAgentPool::from_init(self.init)
    }
}

pub(crate) struct WorkerState {
    pub(crate) init: WorkerInit,
    pub(crate) pool: PoolHandle,
    pub(crate) threads: HashMap<Uuid, ThreadRunner>,
}

pub(crate) struct ThreadRunner {
    pub(crate) agent: Option<BaseAgent>,
    pub(crate) cancel_tx: Option<watch::Sender<bool>>,
    pub(crate) pending_approvals: HashMap<Uuid, PendingApprovalState>,
    pub(crate) pending_quizzes: HashMap<Uuid, PendingQuizState>,
    /// Requests received while a run is in progress, started in order once the thread goes idle.
    pub(crate) queued: VecDeque<(RunId, String, Vec<llm::ImageSource>, Option<String>)>,
    pub(crate) last_event_seq: u64,
    pub(crate) next_message_seq: u64,
    pub(crate) status: ThreadStatus,
    pub(crate) in_progress: Option<PartialAgentMessage>,
    pub(crate) message_format: MessageFormat,
    pub(crate) last_used: Instant,
}

pub(crate) struct PendingApprovalState {
    pub(crate) run_id: RunId,
    pub(crate) message: String,
    pub(crate) approved: futures_oneshot::Sender<bool>,
}

pub(crate) struct PendingQuizState {
    pub(crate) run_id: RunId,
    pub(crate) questions: Vec<QuizQuestion>,
    pub(crate) answered: futures_oneshot::Sender<Vec<String>>,
}

impl InProcessAgentPool {
    pub(crate) fn builder(
        db: ConnectionPool,
        config: Arc<AgentConfig>,
        server_config: config::Config,
        cipher: SecretCipher,
    ) -> InProcessAgentPoolBuilder {
        InProcessAgentPoolBuilder {
            init: WorkerInit {
                db,
                config,
                server_config,
                cipher,
                tools: config::Tools::default(),
                mcp_tools: Vec::new(),
                vfs: None,
                telegram_bot_token: None,
                public_url: None,
                github_runtime: None,
                email_service: None,
                google_service: None,
                system_prompt: BASE_SYSTEM_PROMPT.to_string(),
                idle_ttl: DEFAULT_IDLE_TTL,
            },
        }
    }

    pub fn new(
        db: ConnectionPool,
        config: Arc<AgentConfig>,
        server_config: config::Config,
        cipher: SecretCipher,
    ) -> Self {
        Self::builder(db, config, server_config, cipher)
            .system_prompt(BASE_SYSTEM_PROMPT)
            .idle_ttl(DEFAULT_IDLE_TTL)
            .build()
    }

    fn from_init(init: WorkerInit) -> Self {
        // Build every worker's channel before spawning so the shared routing table
        // is complete when each worker starts and can reach any sibling.
        let mut senders = Vec::with_capacity(WORKER_THREADS);
        let mut receivers = Vec::with_capacity(WORKER_THREADS);
        for _ in 0..WORKER_THREADS {
            let (tx, rx) = mpsc::unbounded_channel();
            senders.push(tx);
            receivers.push(rx);
        }
        let pool = PoolHandle {
            senders: Arc::new(senders),
        };
        for (idx, rx) in receivers.into_iter().enumerate() {
            start_worker(idx, init.clone(), rx, pool.clone());
        }

        Self { pool }
    }
}

#[async_trait]
impl AgentPool for InProcessAgentPool {
    async fn send(&self, thread_id: Uuid, request: AgentRequest) -> Result<RunId, AgentPoolError> {
        self.pool.send(thread_id, request).await
    }

    async fn snapshot(&self, thread_id: Uuid) -> Result<ThreadSnapshot, AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.pool
            .worker(thread_id)
            .send(WorkerCommand::Snapshot { thread_id, resp })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn status(&self, thread_id: Uuid) -> Result<ThreadStatus, AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.pool
            .worker(thread_id)
            .send(WorkerCommand::Status { thread_id, resp })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn cancel_run(&self, thread_id: Uuid) -> Result<(), AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.pool
            .worker(thread_id)
            .send(WorkerCommand::Cancel { thread_id, resp })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn resolve_approval(
        &self,
        thread_id: Uuid,
        approval_id: Uuid,
        approved: bool,
    ) -> Result<(), AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.pool
            .worker(thread_id)
            .send(WorkerCommand::ResolveApproval {
                thread_id,
                approval_id,
                approved,
                resp,
            })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn answer_quiz(
        &self,
        thread_id: Uuid,
        quiz_id: Uuid,
        answers: Vec<String>,
    ) -> Result<(), AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.pool
            .worker(thread_id)
            .send(WorkerCommand::AnswerQuiz {
                thread_id,
                quiz_id,
                answers,
                resp,
            })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }

    async fn shutdown_thread(&self, thread_id: Uuid) -> Result<(), AgentPoolError> {
        let (resp, rx) = oneshot::channel();
        self.pool
            .worker(thread_id)
            .send(WorkerCommand::ShutdownThread { thread_id, resp })
            .map_err(|_| AgentPoolError::WorkerStopped)?;
        rx.await.map_err(|_| AgentPoolError::WorkerStopped)?
    }
}

fn start_worker(
    idx: usize,
    init: WorkerInit,
    rx: mpsc::UnboundedReceiver<WorkerCommand>,
    pool: PoolHandle,
) {
    std::thread::Builder::new()
        .name(format!("stride-agent-pool-{idx}"))
        .spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("agent worker runtime");
            let local = LocalSet::new();
            let state = Rc::new(RefCell::new(WorkerState {
                init,
                pool,
                threads: HashMap::new(),
            }));

            local.block_on(&runtime, run_worker(state, rx));
        })
        .expect("agent worker thread");
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
        WorkerCommand::Snapshot { thread_id, resp } => {
            let result = handle_snapshot(state, thread_id).await;
            let _ = resp.send(result);
        }
        WorkerCommand::Cancel { thread_id, resp } => {
            let result = handle_cancel(&state, thread_id);
            let _ = resp.send(result);
        }
        WorkerCommand::ResolveApproval {
            thread_id,
            approval_id,
            approved,
            resp,
        } => {
            let result = handle_resolve_approval(&state, thread_id, approval_id, approved).await;
            let _ = resp.send(result);
        }
        WorkerCommand::AnswerQuiz {
            thread_id,
            quiz_id,
            answers,
            resp,
        } => {
            let result = handle_answer_quiz(&state, thread_id, quiz_id, answers).await;
            let _ = resp.send(result);
        }
        WorkerCommand::Status { thread_id, resp } => {
            let result = handle_status(state, thread_id).await;
            let _ = resp.send(result);
        }
        WorkerCommand::ShutdownThread { thread_id, resp } => {
            if state.borrow_mut().threads.remove(&thread_id).is_some() {
                deactivate_thread(thread_id);
            }
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

    let (db, clock, id_gen) = {
        let state = state.borrow();
        (
            state.init.db.clone(),
            state.init.config.clock.clone(),
            state.init.config.id_gen.clone(),
        )
    };

    let run_id = RunId(id_gen.new_uuid_v7());

    let user_message_seq = next_message_seq(&state, thread_id)?;
    let user_message_id = id_gen.new_uuid_v7();

    let images_json = (!request.images.is_empty())
        .then(|| serde_json::to_string(&request.images))
        .transpose()
        .map_err(|e| AgentPoolError::Internal(anyhow::anyhow!(e)))?;

    messages::insert()
        .id(user_message_id)
        .parent_thread(thread_id)
        .seq(user_message_seq)
        .role(Role::User)
        .content(request.content.as_str())
        .content_format(MessageFormat::Markdown)
        .images(images_json.as_deref())
        .thinking(Option::<&str>::None)
        .tool_calls(Option::<&str>::None)
        .tool_call_id(Option::<&str>::None)
        .execute(&db)
        .await
        .map_err(db_error)?;

    // Every inbound message (web, Telegram, tools) funnels through here, so this
    // is the single place a thread's last activity is stamped for retention.
    let now_ms = clock.now_unix_millis();
    if let Err(error) = threads::update()
        .last_activity_at(Some(now_ms))
        .where_(threads::id.eq(thread_id))
        .execute(&db)
        .await
    {
        tracing::warn!(%thread_id, %error, "failed to stamp thread activity");
    }

    emit(
        &state,
        thread_id,
        Some(run_id),
        AgentEventKind::UserMessageCommitted {
            message_id: user_message_id,
            seq: user_message_seq,
        },
    )
    .await;

    // Start immediately when idle, otherwise queue and run after the current turn finishes.
    let start_now = {
        let mut state = state.borrow_mut();
        let runner = state
            .threads
            .get_mut(&thread_id)
            .ok_or(AgentPoolError::ThreadNotFound)?;
        runner.last_used = clock.now_instant();
        matches!(runner.status, ThreadStatus::Idle)
    };

    if start_now {
        start_run(
            &state,
            thread_id,
            run_id,
            request.content,
            request.images,
            request.model,
        )
        .await;
    } else {
        with_runner(&state, thread_id, |runner| {
            runner
                .queued
                .push_back((run_id, request.content, request.images, request.model));
        });
    }

    Ok(run_id)
}

/// Marks the thread running, emits `RunStarted`, and spawns the turn. The caller must have already
/// confirmed the thread is idle.
async fn start_run(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    run_id: RunId,
    content: String,
    images: Vec<llm::ImageSource>,
    model: Option<String>,
) {
    let clock = state.borrow().init.config.clock.clone();
    let cancel_rx = {
        let mut state = state.borrow_mut();
        let Some(runner) = state.threads.get_mut(&thread_id) else {
            return;
        };
        let (cancel_tx, cancel_rx) = watch::channel(false);
        runner.cancel_tx = Some(cancel_tx);
        runner.status = ThreadStatus::Running { run_id };
        runner.last_used = clock.now_instant();
        cancel_rx
    };

    emit(state, thread_id, Some(run_id), AgentEventKind::RunStarted).await;
    tokio::task::spawn_local(run_agent_turn(
        state.clone(),
        thread_id,
        run_id,
        content,
        images,
        model,
        cancel_rx,
    ));
}

/// Starts the next queued request if the thread is idle. Called whenever a run ends.
pub(crate) async fn drain_queue(state: &Rc<RefCell<WorkerState>>, thread_id: Uuid) {
    let next = {
        let mut state = state.borrow_mut();
        let Some(runner) = state.threads.get_mut(&thread_id) else {
            return;
        };
        if !matches!(runner.status, ThreadStatus::Idle) {
            return;
        }
        runner.queued.pop_front()
    };

    if let Some((run_id, content, images, model)) = next {
        start_run(state, thread_id, run_id, content, images, model).await;
    }
}

async fn handle_snapshot(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Result<ThreadSnapshot, AgentPoolError> {
    ensure_runner(state.clone(), thread_id).await?;

    let clock = state.borrow().init.config.clock.clone();
    let mut state = state.borrow_mut();
    let runner = state
        .threads
        .get_mut(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;

    runner.last_used = clock.now_instant();
    Ok(ThreadSnapshot {
        thread_id,
        last_event_seq: runner.last_event_seq,
        status: runner.status.clone(),
        in_progress: runner.in_progress.clone(),
        pending_approval: runner
            .pending_approvals
            .iter()
            .next()
            .map(|(approval_id, approval)| PendingApproval {
                approval_id: *approval_id,
                message: approval.message.clone(),
            }),
        pending_quiz: runner
            .pending_quizzes
            .iter()
            .next()
            .map(|(quiz_id, quiz)| PendingQuiz {
                quiz_id: *quiz_id,
                questions: quiz.questions.clone(),
            }),
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

async fn handle_resolve_approval(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    approval_id: Uuid,
    approved: bool,
) -> Result<(), AgentPoolError> {
    let run_id = {
        let mut state = state.borrow_mut();
        let runner = state
            .threads
            .get_mut(&thread_id)
            .ok_or(AgentPoolError::ThreadNotFound)?;
        let Some(approval) = runner.pending_approvals.remove(&approval_id) else {
            return Err(AgentPoolError::ApprovalNotFound);
        };
        let run_id = approval.run_id;
        let _ = approval.approved.send(approved);
        run_id
    };
    emit(
        state,
        thread_id,
        Some(run_id),
        AgentEventKind::ApprovalResolved {
            approval_id,
            approved,
        },
    )
    .await;
    Ok(())
}

async fn handle_answer_quiz(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    quiz_id: Uuid,
    answers: Vec<String>,
) -> Result<(), AgentPoolError> {
    let run_id = {
        let mut state = state.borrow_mut();
        let runner = state
            .threads
            .get_mut(&thread_id)
            .ok_or(AgentPoolError::ThreadNotFound)?;
        let Some(quiz) = runner.pending_quizzes.remove(&quiz_id) else {
            return Err(AgentPoolError::QuizNotFound);
        };
        let run_id = quiz.run_id;
        let _ = quiz.answered.send(answers);
        run_id
    };
    emit(
        state,
        thread_id,
        Some(run_id),
        AgentEventKind::QuizAnswered { quiz_id },
    )
    .await;
    Ok(())
}

async fn handle_status(
    state: Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
) -> Result<ThreadStatus, AgentPoolError> {
    ensure_runner(state.clone(), thread_id).await?;

    let clock = state.borrow().init.config.clock.clone();
    let mut state = state.borrow_mut();
    let runner = state
        .threads
        .get_mut(&thread_id)
        .ok_or(AgentPoolError::ThreadNotFound)?;
    runner.last_used = clock.now_instant();
    Ok(runner.status.clone())
}

fn evict_idle_threads(state: &Rc<RefCell<WorkerState>>) {
    let now = state.borrow().init.config.clock.now_instant();
    let mut state = state.borrow_mut();
    let idle_ttl = state.init.idle_ttl;

    let mut evicted = Vec::new();
    state.threads.retain(|thread_id, runner| {
        let keep = matches!(runner.status, ThreadStatus::Running { .. })
            || now.duration_since(runner.last_used) < idle_ttl;
        if !keep {
            evicted.push(*thread_id);
        }
        keep
    });
    drop(state);

    for thread_id in evicted {
        deactivate_thread(thread_id);
    }
}

/// Tears down a thread's pub/sub presence: drops its event topic (so subscribers observe
/// `Closed`) and announces `Deactivated` so the Telegram supervisor aborts its subscriber task.
fn deactivate_thread(thread_id: Uuid) {
    pubsub::remove(&thread_events_topic(thread_id));
    let _ = pubsub::topic::<RunnerLifecycle>(RUNNER_LIFECYCLE_TOPIC)
        .publish(&RunnerLifecycle::Deactivated { thread_id });
}

pub(crate) fn next_message_seq(
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

pub(crate) fn with_runner(
    state: &Rc<RefCell<WorkerState>>,
    thread_id: Uuid,
    f: impl FnOnce(&mut ThreadRunner),
) {
    if let Some(runner) = state.borrow_mut().threads.get_mut(&thread_id) {
        f(runner);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use llm::{CompletionChoice, Delta, StreamResponseChunk};
    use minisql::Value;
    use stride_agent::{AgentConfig, ModelRegistry};

    use super::*;
    use crate::db::{self, threads, users};
    use crate::runner::{AgentEvent, AgentEventKind, AgentPool, thread_events_topic};

    fn subscribe_events(thread_id: Uuid) -> pubsub::Subscriber<AgentEvent> {
        pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).subscribe()
    }

    fn test_server_config() -> config::Config {
        config::Config {
            providers: HashMap::new(),
            models: HashMap::new(),
            server: None,
            tools: None,
            mcp: HashMap::new(),
        }
    }

    fn test_worker_init(db: ConnectionPool) -> WorkerInit {
        WorkerInit {
            db,
            config: Arc::new(AgentConfig {
                model_registry: ModelRegistry::new(),
                max_iterations: 4,
                observer: Arc::new(stride_agent::NoopAgentObserver),
                ..Default::default()
            }),
            server_config: test_server_config(),
            cipher: SecretCipher::new("test-secret"),
            tools: config::Tools::default(),
            mcp_tools: Vec::new(),
            vfs: None,
            telegram_bot_token: None,
            public_url: None,
            github_runtime: None,
            email_service: None,
            google_service: None,
            system_prompt: "System prompt".to_string(),
            idle_ttl: Duration::from_secs(60),
        }
    }

    fn text_chunk(content: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "mock-stream-id".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            usage: None,
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

    #[tokio::test]
    async fn resolving_approval_updates_state_and_answers_sender() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let thread_id = Uuid::now_v7();
        let run_id = RunId(Uuid::now_v7());
        let approval_id = Uuid::now_v7();
        let (approved_tx, approved_rx) = futures::channel::oneshot::channel();
        let mut events = subscribe_events(thread_id);

        let mut runner = ThreadRunner {
            agent: None,
            cancel_tx: None,
            pending_approvals: HashMap::new(),
            pending_quizzes: HashMap::new(),
            queued: VecDeque::new(),
            last_event_seq: 0,
            next_message_seq: 0,
            status: ThreadStatus::Running { run_id },
            in_progress: None,
            message_format: MessageFormat::Html,
            last_used: Instant::now(),
        };
        runner.pending_approvals.insert(
            approval_id,
            PendingApprovalState {
                run_id,
                message: "Approve test".to_string(),
                approved: approved_tx,
            },
        );

        let mut threads = HashMap::new();
        threads.insert(thread_id, runner);
        let state = Rc::new(RefCell::new(WorkerState {
            init: test_worker_init(db.clone()),
            pool: PoolHandle::for_tests(),
            threads,
        }));

        let snapshot = handle_snapshot(state.clone(), thread_id).await.unwrap();
        assert_eq!(
            snapshot.pending_approval.as_ref().map(|a| a.approval_id),
            Some(approval_id)
        );

        handle_resolve_approval(&state, thread_id, approval_id, false)
            .await
            .unwrap();
        assert!(!approved_rx.await.unwrap());

        let event = events.recv().await.unwrap();
        match event.kind {
            AgentEventKind::ApprovalResolved {
                approval_id: resolved_id,
                approved,
            } => {
                assert_eq!(resolved_id, approval_id);
                assert!(!approved);
            }
            _ => panic!("expected approval resolution"),
        }

        let snapshot = handle_snapshot(state, thread_id).await.unwrap();
        assert!(snapshot.pending_approval.is_none());
    }

    #[tokio::test]
    async fn answering_quiz_updates_state_and_answers_sender() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let thread_id = Uuid::now_v7();
        let run_id = RunId(Uuid::now_v7());
        let quiz_id = Uuid::now_v7();
        let questions = vec![QuizQuestion {
            question: "Pick one".to_string(),
            options: vec!["A".to_string(), "B".to_string()],
        }];
        let (answered_tx, answered_rx) = futures::channel::oneshot::channel();
        let mut events = subscribe_events(thread_id);

        let mut runner = ThreadRunner {
            agent: None,
            cancel_tx: None,
            pending_approvals: HashMap::new(),
            pending_quizzes: HashMap::new(),
            queued: VecDeque::new(),
            last_event_seq: 0,
            next_message_seq: 0,
            status: ThreadStatus::Running { run_id },
            in_progress: None,
            message_format: MessageFormat::Html,
            last_used: Instant::now(),
        };
        runner.pending_quizzes.insert(
            quiz_id,
            PendingQuizState {
                run_id,
                questions: questions.clone(),
                answered: answered_tx,
            },
        );

        let mut threads = HashMap::new();
        threads.insert(thread_id, runner);
        let state = Rc::new(RefCell::new(WorkerState {
            init: test_worker_init(db.clone()),
            pool: PoolHandle::for_tests(),
            threads,
        }));

        let snapshot = handle_snapshot(state.clone(), thread_id).await.unwrap();
        assert_eq!(
            snapshot.pending_quiz.as_ref().map(|q| q.quiz_id),
            Some(quiz_id)
        );
        assert_eq!(snapshot.pending_quiz.unwrap().questions, questions);

        handle_answer_quiz(&state, thread_id, quiz_id, vec!["B".to_string()])
            .await
            .unwrap();
        assert_eq!(answered_rx.await.unwrap(), vec!["B".to_string()]);

        let event = events.recv().await.unwrap();
        match event.kind {
            AgentEventKind::QuizAnswered {
                quiz_id: answered_id,
            } => assert_eq!(answered_id, quiz_id),
            _ => panic!("expected quiz answer"),
        }

        let snapshot = handle_snapshot(state, thread_id).await.unwrap();
        assert!(snapshot.pending_quiz.is_none());
    }

    #[tokio::test]
    async fn concurrent_sends_queue_and_run_in_order() {
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
            stride_agent::DEFAULT_MODEL,
            stride_agent::ModelRegEntry {
                api: llm::Mock::new()
                    .with_stream_chunks(vec![
                        vec![text_chunk("one")],
                        vec![text_chunk("two")],
                        vec![text_chunk("three")],
                    ])
                    .into(),
                token: "-".to_string(),
                model_name: "mock-model".to_string(),
                reasoning_effort: None,
                vision: false,
            },
        );

        let pool = InProcessAgentPool::builder(
            db.clone(),
            Arc::new(AgentConfig {
                model_registry: models,
                max_iterations: 4,
                observer: Arc::new(stride_agent::NoopAgentObserver),
                ..Default::default()
            }),
            test_server_config(),
            SecretCipher::new("test-secret"),
        )
        .system_prompt("System prompt")
        .idle_ttl(Duration::from_secs(60))
        .build();

        // Fire three sends back-to-back; the second and third must queue, not be rejected.
        for content in ["msg0", "msg1", "msg2"] {
            pool.send(
                thread_id,
                AgentRequest {
                    content: content.to_string(),
                    images: Vec::new(),
                    model: None,
                },
            )
            .await
            .unwrap();
        }

        for _ in 0..200 {
            let rows = db
                .query_with_params(
                    "SELECT COUNT(*) AS n FROM messages WHERE parent_thread = ? AND role = 'user'",
                    vec![Value::Uuid(thread_id)],
                )
                .await
                .unwrap();
            if rows.rows().first().and_then(|r| r.get_int("n")) == Some(3)
                && pool.status(thread_id).await.unwrap() == ThreadStatus::Idle
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let rows = db
            .query_with_params(
                "SELECT content FROM messages WHERE parent_thread = ? AND role = 'user' ORDER BY seq ASC",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        let order: Vec<_> = rows
            .rows()
            .iter()
            .filter_map(|r| r.get_text("content").map(str::to_string))
            .collect();
        assert_eq!(order, vec!["msg0", "msg1", "msg2"]);
    }
}
