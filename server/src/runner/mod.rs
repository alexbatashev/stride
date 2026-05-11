mod inproc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use uuid::Uuid;

pub type EventSeq = u64;

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
}

#[derive(Clone, Debug)]
pub struct ThreadSnapshot {
    pub thread_id: Uuid,
    pub last_event_seq: EventSeq,
    pub status: ThreadStatus,
    pub in_progress: Option<PartialAgentMessage>,
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
    UserMessageCommitted { message_id: Uuid, seq: u64 },
    AgentDelta { content: String },
    ThinkingDelta { thinking: String },
    AgentMessageCommitted { message_id: Uuid, seq: u64 },
    ToolStarted { name: String },
    ToolFinished { name: String },
    WaitingForApproval { approval_id: Uuid, message: String },
    RunFinished,
    RunFailed { error: String },
}

#[derive(Debug)]
pub enum AgentPoolError {
    ThreadNotFound,
    AlreadyRunning,
    EventHistoryExpired,
    Internal(anyhow::Error),
}
