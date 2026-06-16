use async_trait::async_trait;
use friday_agent::QuizQuestion;
use minisql::ConnectionPool;
use tokio::sync::broadcast;
use uuid::Uuid;

pub mod inproc;

pub type EventSeq = u64;

/// A consumer of a thread's agent events. The agent holds one of these per attached sink and
/// presents every emitted event to each. `dispatch` runs on the agent worker thread, so it may
/// call out to the network directly, but it must not call back into the [`AgentPool`] (the worker
/// drives a single-threaded runtime — a re-entrant pool call would deadlock).
#[async_trait(?Send)]
pub trait EventDispatcher {
    async fn dispatch(&self, event: &AgentEvent);
}

/// Builds source-specific dispatchers when a thread runner first starts. Returning `None` means
/// the factory does not apply to this thread (e.g. it was not created in Telegram).
#[async_trait]
pub trait DispatcherFactory: Send + Sync + 'static {
    async fn make(&self, thread_id: Uuid, db: &ConnectionPool) -> Option<Box<dyn EventDispatcher>>;
}

#[async_trait]
pub trait AgentPool: Send + Sync + 'static {
    async fn send(&self, thread_id: Uuid, request: AgentRequest) -> Result<RunId, AgentPoolError>;

    /// Implementations must create the receiver and snapshot under the same lock.
    /// Otherwise reconnecting clients can miss events between replay and live stream.
    async fn subscribe(
        &self,
        thread_id: Uuid,
        after: Option<EventSeq>,
    ) -> Result<ThreadSubscription, AgentPoolError>;

    async fn cancel_run(&self, thread_id: Uuid) -> Result<(), AgentPoolError>;

    async fn resolve_approval(
        &self,
        thread_id: Uuid,
        approval_id: Uuid,
        approved: bool,
    ) -> Result<(), AgentPoolError>;

    async fn answer_quiz(
        &self,
        thread_id: Uuid,
        quiz_id: Uuid,
        answers: Vec<String>,
    ) -> Result<(), AgentPoolError>;

    async fn status(&self, thread_id: Uuid) -> Result<ThreadStatus, AgentPoolError>;

    async fn shutdown_thread(&self, thread_id: Uuid) -> Result<(), AgentPoolError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRequest {
    pub content: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RunId(pub Uuid);

pub struct ThreadSubscription {
    pub snapshot: ThreadSnapshot,
    pub events: broadcast::Receiver<AgentEvent>,
    pub replay: Vec<AgentEvent>,
}

#[derive(Clone, Debug)]
pub struct ThreadSnapshot {
    pub thread_id: Uuid,
    pub last_event_seq: EventSeq,
    pub status: ThreadStatus,
    pub in_progress: Option<PartialAgentMessage>,
    pub pending_approval: Option<PendingApproval>,
    pub pending_quiz: Option<PendingQuiz>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadStatus {
    Idle,
    Running { run_id: RunId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialAgentMessage {
    pub run_id: RunId,
    pub content: String,
    pub thinking: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingApproval {
    pub approval_id: Uuid,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingQuiz {
    pub quiz_id: Uuid,
    pub questions: Vec<QuizQuestion>,
}

#[derive(Clone, Debug)]
pub struct AgentEvent {
    pub seq: EventSeq,
    pub thread_id: Uuid,
    pub run_id: Option<RunId>,
    pub kind: AgentEventKind,
}

#[derive(Clone, Debug)]
pub enum AgentEventKind {
    RunStarted,
    UserMessageCommitted {
        message_id: Uuid,
        seq: u64,
    },
    AgentDelta {
        content: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    AgentMessageCommitted {
        message_id: Uuid,
        seq: u64,
    },
    ToolStarted {
        name: String,
    },
    ToolFinished {
        name: String,
    },
    WaitingForApproval {
        approval_id: Uuid,
        message: String,
    },
    ApprovalResolved {
        approval_id: Uuid,
        approved: bool,
    },
    WaitingForQuiz {
        quiz_id: Uuid,
        questions: Vec<QuizQuestion>,
    },
    QuizAnswered {
        quiz_id: Uuid,
    },
    RunFinished,
    RunFailed {
        error: String,
    },
    RunCancelled,
}

#[derive(Debug)]
pub enum AgentPoolError {
    ThreadNotFound,
    AlreadyRunning,
    ApprovalNotFound,
    QuizNotFound,
    EventHistoryExpired,
    WorkerStopped,
    Internal(anyhow::Error),
}

impl std::fmt::Display for AgentPoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentPoolError::ThreadNotFound => write!(f, "thread not found"),
            AgentPoolError::AlreadyRunning => write!(f, "thread is already running"),
            AgentPoolError::ApprovalNotFound => write!(f, "approval not found"),
            AgentPoolError::QuizNotFound => write!(f, "quiz not found"),
            AgentPoolError::EventHistoryExpired => write!(f, "event history expired"),
            AgentPoolError::WorkerStopped => write!(f, "agent worker stopped"),
            AgentPoolError::Internal(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AgentPoolError {}
