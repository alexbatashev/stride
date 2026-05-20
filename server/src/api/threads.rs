use std::sync::Arc;

use axum::{
    Json,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use minisql::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use friday_agent::DEFAULT_MODEL;
use llm::{CompletionRequest, Message as LlmMessage, Role as LlmRole};

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::{Role, messages, threads},
    runner::{
        AgentEvent, AgentEventKind, AgentPoolError, AgentRequest, RunId, ThreadStatus,
        ThreadSubscription,
    },
};

#[derive(Serialize)]
pub struct ThreadResponse {
    id: String,
    title: String,
}

#[derive(Serialize)]
pub struct MessageResponse {
    id: String,
    seq: u64,
    role: &'static str,
    content: String,
    thinking: Option<String>,
    tool_call_name: Option<String>,
}

#[derive(Serialize)]
pub struct ThreadPageData {
    thread_id: String,
    current_title: String,
    running: bool,
    threads: Vec<ThreadTemplateData>,
    messages: Vec<MessageTemplateData>,
}

#[derive(Serialize)]
struct ThreadTemplateData {
    id: String,
    title: String,
    active: bool,
}

#[derive(Serialize)]
struct MessageTemplateData {
    id: String,
    seq: u64,
    role: &'static str,
    message_type: &'static str,
    tool_name: Option<String>,
    content: String,
    thinking: Option<String>,
    has_thinking: bool,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    content: String,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    after: Option<u64>,
}

#[derive(Serialize)]
pub struct SendMessageResponse {
    thread_id: String,
    run_id: String,
}

#[derive(Serialize)]
struct EventResponse {
    seq: u64,
    thread_id: String,
    run_id: Option<String>,
    kind: EventKindResponse,
}

#[derive(Serialize)]
struct SnapshotMessageResponse {
    run_id: String,
    content: String,
    thinking: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum EventKindResponse {
    Snapshot {
        status: &'static str,
        in_progress: Option<SnapshotMessageResponse>,
    },
    RunStarted,
    UserMessageCommitted {
        message_id: String,
        seq: u64,
    },
    AgentDelta {
        content: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    AgentMessageCommitted {
        message_id: String,
        seq: u64,
    },
    ToolStarted {
        name: String,
    },
    ToolFinished {
        name: String,
    },
    WaitingForApproval {
        approval_id: String,
        message: String,
    },
    RunFinished,
    RunFailed {
        error: String,
    },
    RunCancelled,
}

#[derive(Debug)]
pub enum ThreadApiError {
    Auth(AuthError),
    BadRequest,
    NotFound,
    Conflict,
    Internal,
}

impl IntoResponse for ThreadApiError {
    fn into_response(self) -> Response {
        match self {
            ThreadApiError::Auth(error) => error.into_response(),
            ThreadApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            ThreadApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            ThreadApiError::Conflict => StatusCode::CONFLICT.into_response(),
            ThreadApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for ThreadApiError {
    fn from(error: AuthError) -> Self {
        ThreadApiError::Auth(error)
    }
}

pub async fn list_threads(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ThreadResponse>>, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    Ok(Json(thread_summaries(&state, owner).await?))
}

pub async fn thread_page_data(
    state: &ServerState,
    headers: &HeaderMap,
    thread_id: Option<Uuid>,
) -> Result<ThreadPageData, ThreadApiError> {
    let owner = auth::authenticated_user(state, headers).await?;
    let threads = thread_summaries(state, owner).await?;
    let current_title = thread_id
        .and_then(|id| {
            threads
                .iter()
                .find(|thread| thread.id == id.to_string())
                .map(|thread| thread.title.clone())
        })
        .unwrap_or_else(|| "New thread".to_string());

    let (messages, running) = if let Some(thread_id) = thread_id {
        require_thread_owner_for_user(state, owner, thread_id).await?;
        (
            thread_messages(state, thread_id).await?,
            matches!(
                state.runner.status(thread_id).await.map_err(pool_error)?,
                ThreadStatus::Running { .. }
            ),
        )
    } else {
        (Vec::new(), false)
    };

    Ok(ThreadPageData {
        thread_id: thread_id.map(|id| id.to_string()).unwrap_or_default(),
        current_title,
        running,
        threads: threads
            .into_iter()
            .map(|thread| ThreadTemplateData {
                active: thread_id
                    .map(|id| thread.id == id.to_string())
                    .unwrap_or(false),
                id: thread.id,
                title: thread.title,
            })
            .collect(),
        messages: messages
            .into_iter()
            .map(|message| {
                let (message_type, tool_name) = message_template_type(&message);
                let has_thinking = message.thinking.is_some();
                MessageTemplateData {
                    id: message.id,
                    seq: message.seq,
                    role: message.role,
                    message_type,
                    tool_name,
                    content: message.content,
                    thinking: message.thinking,
                    has_thinking,
                }
            })
            .collect(),
    })
}

async fn thread_summaries(
    state: &ServerState,
    owner: Uuid,
) -> Result<Vec<ThreadResponse>, ThreadApiError> {
    let result = state
        .db
        .query_with_params(
            "SELECT id, title FROM threads WHERE owner = ? ORDER BY id DESC",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    let mut threads = Vec::new();
    for row in result.rows() {
        threads.push(ThreadResponse {
            id: uuid_value(row.get("id"))?.to_string(),
            title: row.get_text("title").unwrap_or("Untitled").to_string(),
        });
    }

    Ok(threads)
}

pub async fn create_thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let content = normalize_content(request.content)?;
    let thread_id = Uuid::now_v7();
    let title = title_from_content(&content);

    threads::insert()
        .id(thread_id)
        .owner(owner)
        .title(title.as_str())
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    let run_id = send_to_runner(&state, thread_id, content.clone()).await?;

    let db = state.db.clone();
    let model_config = state.model_config.clone();
    tokio::spawn(async move {
        if let Some(title) = generate_title(&model_config, &content).await {
            let _ = db
                .query_with_params(
                    "UPDATE threads SET title = ? WHERE id = ?",
                    vec![Value::Text(title), Value::Uuid(thread_id)],
                )
                .await;
        }
    });

    Ok(Json(SendMessageResponse {
        thread_id: thread_id.to_string(),
        run_id: run_id.0.to_string(),
    }))
}

async fn generate_title(
    config: &std::sync::Arc<friday_agent::AgentConfig>,
    content: &str,
) -> Option<String> {
    let model = config.model_registry.get_or_default(DEFAULT_MODEL);
    let prompt = format!(
        "Generate a concise title (5-8 words) for a conversation that starts with this message. Return only the title, no quotes or trailing punctuation.\n\nMessage: {content}"
    );
    let request = CompletionRequest::new(
        &model.model_name,
        &[LlmMessage {
            role: LlmRole::User,
            content: prompt,
            ..Default::default()
        }],
    )
    .max_tokens(32);

    model
        .api
        .get_completion(&model.token, request)
        .await
        .ok()
        .and_then(|c| c.choices.into_iter().next())
        .and_then(|choice| choice.message)
        .map(|msg| msg.content.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub async fn list_messages(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<Json<Vec<MessageResponse>>, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    Ok(Json(thread_messages(&state, thread_id).await?))
}

async fn thread_messages(
    state: &ServerState,
    thread_id: Uuid,
) -> Result<Vec<MessageResponse>, ThreadApiError> {
    let rows = messages::select()
        .where_(messages::parent_thread.eq(thread_id))
        .order_by_asc(messages::seq)
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    Ok(rows
        .into_iter()
        .map(|row| MessageResponse {
            id: row.id.to_string(),
            seq: row.seq,
            role: role_name(row.role),
            content: row.content,
            thinking: row.thinking,
            tool_call_name: tool_call_name(row.tool_calls.as_deref()),
        })
        .filter(|message| {
            message.role != "agent"
                || !message.content.is_empty()
                || message.thinking.is_some()
                || message.tool_call_name.is_some()
        })
        .collect())
}

pub async fn send_message(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    let content = normalize_content(request.content)?;
    let run_id = send_to_runner(&state, thread_id, content).await?;

    Ok(Json(SendMessageResponse {
        thread_id: thread_id.to_string(),
        run_id: run_id.0.to_string(),
    }))
}

pub async fn events(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<EventsQuery>,
) -> Result<Response, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    let subscription = state
        .runner
        .subscribe(thread_id, query.after)
        .await
        .map_err(pool_error)?;

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, subscription)))
}

pub async fn cancel(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<StatusCode, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    state
        .runner
        .cancel_run(thread_id)
        .await
        .map_err(pool_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn handle_ws(mut socket: WebSocket, subscription: ThreadSubscription) {
    let snapshot = snapshot_event(&subscription);
    if socket.send(Message::Text(snapshot.into())).await.is_err() {
        return;
    }

    for event in subscription.replay {
        let Ok(data) = serde_json::to_string(&event_response(event)) else {
            return;
        };
        if socket.send(Message::Text(data.into())).await.is_err() {
            return;
        }
    }

    let mut events = subscription.events;
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        let Ok(data) = serde_json::to_string(&event_response(event)) else {
                            break;
                        };
                        if socket.send(Message::Text(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn require_thread_owner(
    state: &ServerState,
    headers: &HeaderMap,
    thread_id: Uuid,
) -> Result<(), ThreadApiError> {
    let owner = auth::authenticated_user(state, headers).await?;
    require_thread_owner_for_user(state, owner, thread_id).await
}

async fn require_thread_owner_for_user(
    state: &ServerState,
    owner: Uuid,
    thread_id: Uuid,
) -> Result<(), ThreadApiError> {
    let rows = threads::select_cols((threads::id,))
        .where_(threads::id.eq(thread_id).and(threads::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    if rows.is_empty() {
        Err(ThreadApiError::NotFound)
    } else {
        Ok(())
    }
}

async fn send_to_runner(
    state: &ServerState,
    thread_id: Uuid,
    content: String,
) -> Result<RunId, ThreadApiError> {
    state
        .runner
        .send(thread_id, AgentRequest { content })
        .await
        .map_err(pool_error)
}

fn normalize_content(content: String) -> Result<String, ThreadApiError> {
    let content = content.trim().to_string();
    if content.is_empty() {
        Err(ThreadApiError::BadRequest)
    } else {
        Ok(content)
    }
}

fn title_from_content(content: &str) -> String {
    let title: String = content.chars().take(64).collect();
    if title.len() < content.len() {
        format!("{title}...")
    } else {
        title
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::Agent => "agent",
        Role::User => "user",
        Role::Tool => "tool",
    }
}

fn message_template_type(message: &MessageResponse) -> (&'static str, Option<String>) {
    if let Some(name) = &message.tool_call_name {
        return ("tool_call", Some(name.clone()));
    }

    match message.role {
        "tool" => ("tool_output", Some("Tool output".to_string())),
        "system" => ("agent", None),
        _ => (message.role, None),
    }
}

fn tool_call_name(tool_calls: Option<&str>) -> Option<String> {
    let calls: Vec<llm::ToolCallChunk> = serde_json::from_str(tool_calls?).ok()?;
    calls
        .first()
        .and_then(|call| call.function.as_ref())
        .and_then(|function| function.name.clone())
}

fn uuid_value(value: Option<&Value>) -> Result<Uuid, ThreadApiError> {
    match value {
        Some(Value::Uuid(id)) => Ok(*id),
        Some(Value::Blob(bytes)) if bytes.len() == 16 => {
            Uuid::from_slice(bytes).map_err(|_| ThreadApiError::Internal)
        }
        Some(Value::Text(id)) => Uuid::parse_str(id).map_err(|_| ThreadApiError::Internal),
        _ => Err(ThreadApiError::Internal),
    }
}

fn pool_error(error: AgentPoolError) -> ThreadApiError {
    match error {
        AgentPoolError::ThreadNotFound => ThreadApiError::NotFound,
        AgentPoolError::AlreadyRunning => ThreadApiError::Conflict,
        AgentPoolError::EventHistoryExpired
        | AgentPoolError::WorkerStopped
        | AgentPoolError::Internal(_) => ThreadApiError::Internal,
    }
}

fn event_response(event: AgentEvent) -> EventResponse {
    EventResponse {
        seq: event.seq,
        thread_id: event.thread_id.to_string(),
        run_id: event.run_id.map(|run_id| run_id.0.to_string()),
        kind: match event.kind {
            AgentEventKind::RunStarted => EventKindResponse::RunStarted,
            AgentEventKind::UserMessageCommitted { message_id, seq } => {
                EventKindResponse::UserMessageCommitted {
                    message_id: message_id.to_string(),
                    seq,
                }
            }
            AgentEventKind::AgentDelta { content } => EventKindResponse::AgentDelta { content },
            AgentEventKind::ThinkingDelta { thinking } => {
                EventKindResponse::ThinkingDelta { thinking }
            }
            AgentEventKind::AgentMessageCommitted { message_id, seq } => {
                EventKindResponse::AgentMessageCommitted {
                    message_id: message_id.to_string(),
                    seq,
                }
            }
            AgentEventKind::ToolStarted { name } => EventKindResponse::ToolStarted { name },
            AgentEventKind::ToolFinished { name } => EventKindResponse::ToolFinished { name },
            AgentEventKind::WaitingForApproval {
                approval_id,
                message,
            } => EventKindResponse::WaitingForApproval {
                approval_id: approval_id.to_string(),
                message,
            },
            AgentEventKind::RunFinished => EventKindResponse::RunFinished,
            AgentEventKind::RunFailed { error } => EventKindResponse::RunFailed { error },
            AgentEventKind::RunCancelled => EventKindResponse::RunCancelled,
        },
    }
}

fn snapshot_event(subscription: &crate::runner::ThreadSubscription) -> String {
    let event = EventResponse {
        seq: subscription.snapshot.last_event_seq,
        thread_id: subscription.snapshot.thread_id.to_string(),
        run_id: None,
        kind: EventKindResponse::Snapshot {
            status: match subscription.snapshot.status {
                ThreadStatus::Idle => "idle",
                ThreadStatus::Running { .. } => "running",
            },
            in_progress: subscription.snapshot.in_progress.as_ref().map(|message| {
                SnapshotMessageResponse {
                    run_id: message.run_id.0.to_string(),
                    content: message.content.clone(),
                    thinking: message.thinking.clone(),
                }
            }),
        },
    };

    serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_value_accepts_sqlite_blob_uuid() {
        let id = Uuid::now_v7();
        let value = Value::Blob(id.as_bytes().to_vec());

        assert_eq!(uuid_value(Some(&value)).unwrap(), id);
    }
}
