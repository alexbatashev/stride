use async_trait::async_trait;
use stride_agent::QuizQuestion;
use uuid::Uuid;

use crate::db::MessageFormat;

pub mod inproc;

pub type EventSeq = u64;

/// Global pub/sub topic name carrying [`AgentEvent`]s for one thread. Producers (the worker) and
/// consumers (WS handler, Telegram subscriber) reach the same channel through this name.
pub fn thread_events_topic(thread_id: Uuid) -> String {
    format!("thread-events:{thread_id}")
}

/// Global pub/sub topic announcing when a thread runner is created or evicted. The Telegram
/// supervisor listens here to bind a subscriber task's lifetime to the runner's lifetime.
pub const RUNNER_LIFECYCLE_TOPIC: &str = "runner-lifecycle";

/// Lifecycle of a thread runner, published on [`RUNNER_LIFECYCLE_TOPIC`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerLifecycle {
    Activated { thread_id: Uuid },
    Deactivated { thread_id: Uuid },
}

#[async_trait]
pub trait AgentPool: Send + Sync + 'static {
    async fn send(&self, thread_id: Uuid, request: AgentRequest) -> Result<RunId, AgentPoolError>;

    /// Point-in-time snapshot of a thread. Consumers stream live events from the thread's pub/sub
    /// topic and use [`ThreadSnapshot::last_event_seq`] to discard events already reflected here.
    async fn snapshot(&self, thread_id: Uuid) -> Result<ThreadSnapshot, AgentPoolError>;

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentRequest {
    pub content: String,
    pub images: Vec<llm::ImageSource>,
    pub model: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RunId(pub Uuid);

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
    pub format: MessageFormat,
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
        format: MessageFormat,
    },
    ThinkingDelta {
        thinking: String,
    },
    AgentMessageCommitted {
        message_id: Uuid,
        seq: u64,
    },
    ToolStarted {
        tool_call_id: String,
        name: String,
    },
    /// Incremental human-facing output from a streaming or backgrounded tool.
    ToolProgress {
        tool_call_id: String,
        name: String,
        delta: String,
        format: String,
    },
    ToolFinished {
        tool_call_id: String,
        name: String,
        format: String,
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
