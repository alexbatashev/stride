use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    body::Body,
    extract::{
        Multipart, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use minisql::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use friday_agent::DEFAULT_MODEL;
use llm::{CompletionRequest, Message as LlmMessage, Role as LlmRole};

use crate::{
    ServerState,
    api::{
        auth::{self, AuthError},
        projects::ProjectResponse,
    },
    db::{Role, messages, projects, threads},
    runner::{
        AgentEvent, AgentEventKind, AgentPoolError, AgentRequest, RunId, ThreadSnapshot,
        ThreadStatus, thread_events_topic,
    },
};

/// Placeholder title a new thread gets until the LLM generates a real one.
pub(crate) const DEFAULT_THREAD_TITLE: &str = "New chat";
/// Telegram caps forum topic names at 128 characters; we cap every generated title to match.
const MAX_TITLE_LEN: usize = 128;
const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Serialize)]
pub struct ThreadResponse {
    id: String,
    title: String,
    project_id: Option<String>,
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
    pub thread_id: String,
    pub current_title: String,
    pub running: bool,
    pub projects: Vec<ProjectTemplateData>,
    pub ungrouped_threads: Vec<ThreadTemplateData>,
    pub messages: Vec<MessageTemplateData>,
}

#[derive(Serialize)]
pub struct ProjectTemplateData {
    pub id: String,
    pub title: String,
    pub threads: Vec<ThreadTemplateData>,
}

#[derive(Serialize, Clone)]
pub struct ThreadTemplateData {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub active: bool,
}

#[derive(Serialize)]
pub struct MessageTemplateData {
    pub id: String,
    pub seq: u64,
    pub role: &'static str,
    pub message_type: &'static str,
    pub tool_name: Option<String>,
    pub content: String,
    pub thinking: Option<String>,
    pub has_thinking: bool,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    content: String,
    project_id: Option<Uuid>,
    #[serde(default)]
    file_paths: Vec<String>,
}

#[derive(Deserialize)]
pub struct ApprovalRequest {
    approved: bool,
}

#[derive(Deserialize)]
pub struct QuizAnswerRequest {
    answers: Vec<String>,
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
struct ApprovalResponse {
    approval_id: String,
    message: String,
}

#[derive(Serialize)]
struct QuizQuestionResponse {
    question: String,
    options: Vec<String>,
}

#[derive(Serialize)]
struct QuizResponse {
    quiz_id: String,
    questions: Vec<QuizQuestionResponse>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum EventKindResponse {
    Snapshot {
        status: &'static str,
        in_progress: Option<SnapshotMessageResponse>,
        pending_approval: Option<ApprovalResponse>,
        pending_quiz: Option<QuizResponse>,
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
    ApprovalResolved {
        approval_id: String,
        approved: bool,
    },
    WaitingForQuiz {
        quiz_id: String,
        questions: Vec<QuizQuestionResponse>,
    },
    QuizAnswered {
        quiz_id: String,
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
    let all_threads = thread_summaries(state, owner).await?;
    let all_projects = project_summaries(state, owner).await?;

    let current_title = thread_id
        .and_then(|id| {
            all_threads
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

    let thread_template_data: Vec<ThreadTemplateData> = all_threads
        .iter()
        .map(|thread| ThreadTemplateData {
            active: thread_id
                .map(|id| thread.id == id.to_string())
                .unwrap_or(false),
            id: thread.id.clone(),
            title: thread.title.clone(),
            project_id: thread.project_id.clone(),
        })
        .collect();

    let projects = all_projects
        .into_iter()
        .map(|project| ProjectTemplateData {
            threads: thread_template_data
                .iter()
                .filter(|t| t.project_id.as_deref() == Some(&project.id))
                .cloned()
                .collect(),
            id: project.id,
            title: project.title,
        })
        .collect();

    let ungrouped_threads = thread_template_data
        .into_iter()
        .filter(|t| t.project_id.is_none())
        .collect();

    Ok(ThreadPageData {
        thread_id: thread_id.map(|id| id.to_string()).unwrap_or_default(),
        current_title,
        running,
        projects,
        ungrouped_threads,
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
            "SELECT id, title, project_id FROM threads WHERE owner = ? ORDER BY id DESC",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    let mut threads = Vec::new();
    for row in result.rows() {
        let project_id = match row.get("project_id") {
            Some(Value::Uuid(id)) => Some(id.to_string()),
            Some(Value::Blob(bytes)) if bytes.len() == 16 => {
                Uuid::from_slice(bytes).ok().map(|id| id.to_string())
            }
            _ => None,
        };
        threads.push(ThreadResponse {
            id: uuid_value(row.get("id"))?.to_string(),
            title: row.get_text("title").unwrap_or("Untitled").to_string(),
            project_id,
        });
    }

    Ok(threads)
}

async fn project_summaries(
    state: &ServerState,
    owner: Uuid,
) -> Result<Vec<ProjectResponse>, ThreadApiError> {
    let rows = projects::select()
        .where_(projects::owner.eq(owner))
        .order_by_desc(projects::id)
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    Ok(rows
        .into_iter()
        .map(|r| ProjectResponse {
            id: r.id.to_string(),
            title: r.title,
        })
        .collect())
}

pub async fn create_thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let project_id = request.project_id;
    let thread_id = Uuid::now_v7();

    let mut insert = threads::insert()
        .id(thread_id)
        .owner(owner)
        .title(DEFAULT_THREAD_TITLE);
    if let Some(pid) = project_id {
        insert = insert.project_id(pid);
    }
    insert
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    let (content, images) = prepare_message(
        &state,
        thread_id,
        owner,
        request.content,
        request.file_paths,
    )
    .await?;
    let content = normalize_content(content)?;

    let run_id = send_to_runner_with_images(&state, thread_id, content.clone(), images).await?;

    spawn_title_generation(state.clone(), thread_id, content, None);

    Ok(Json(SendMessageResponse {
        thread_id: thread_id.to_string(),
        run_id: run_id.0.to_string(),
    }))
}

/// The single background task that names a freshly created thread: generate a title from its first
/// message, save it, and—when the thread is a Telegram forum topic—rename that topic too. Used by
/// every thread-creating path so naming behaves identically regardless of origin.
pub(crate) fn spawn_title_generation(
    state: Arc<ServerState>,
    thread_id: Uuid,
    content: String,
    forum_topic: Option<(i64, i64)>,
) {
    tokio::spawn(async move {
        let (title, used_fallback) = match tokio::time::timeout(
            TITLE_GENERATION_TIMEOUT,
            generate_title(&state.model_config, &content),
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                tracing::warn!(%thread_id, %error, "title generation failed");
                return;
            }
            Err(error) => {
                tracing::warn!(%thread_id, %error, "title generation timed out");
                return;
            }
        };
        if used_fallback {
            tracing::warn!(%thread_id, %title, "title generation returned no visible text; using fallback");
        }
        match threads::update()
            .title(title.as_str())
            .where_(threads::id.eq(thread_id))
            .execute(&state.db)
            .await
        {
            Ok(_) => {
                let stored = threads::select_cols((threads::title,))
                    .where_(threads::id.eq(thread_id))
                    .all(&state.db)
                    .await
                    .ok()
                    .and_then(|rows| rows.into_iter().next());
                match stored {
                    Some((stored,)) if stored == title => {}
                    Some((stored,)) => tracing::warn!(
                        %thread_id,
                        %title,
                        %stored,
                        "title update did not stick"
                    ),
                    None => {
                        tracing::warn!(%thread_id, %title, "title update matched no thread row")
                    }
                }
            }
            Err(error) => tracing::warn!(%thread_id, %error, "title update failed"),
        }
        if let Some((chat_id, message_thread_id)) = forum_topic {
            crate::api::telegram::edit_forum_topic(&state, chat_id, message_thread_id, &title)
                .await;
        }
    });
}

async fn generate_title(
    config: &Arc<friday_agent::AgentConfig>,
    content: &str,
) -> Result<(String, bool), llm::Error> {
    let model = config.model_registry.get_or_default(DEFAULT_MODEL);
    let request = title_generation_request(&model.model_name, content);

    let completion = model.api.get_completion(&model.token, request).await?;

    if let Some(title) = title_from_completion(completion) {
        return Ok((title, false));
    }

    Ok((fallback_title(content), true))
}

fn title_generation_request(model_name: &str, content: &str) -> CompletionRequest {
    CompletionRequest::new(
        model_name,
        &[
            LlmMessage {
                role: LlmRole::System,
                content: "Generate a concise title (5-8 words) for a conversation. Return only the title, no quotes or trailing punctuation.".to_string(),
                ..Default::default()
            },
            LlmMessage {
                role: LlmRole::User,
                content: format!("FIRST USER REQUEST:\n{content}"),
                ..Default::default()
            },
        ],
    )
    .max_tokens(1024)
}

fn title_from_completion(completion: llm::Completion) -> Option<String> {
    completion.choices.into_iter().find_map(|choice| {
        let message_content = choice.message.map(|message| message.content);
        [message_content, choice.text]
            .into_iter()
            .flatten()
            .find_map(normalize_title)
    })
}

fn normalize_title(text: String) -> Option<String> {
    let title = text
        .trim()
        .trim_matches(title_edge_punctuation)
        .chars()
        .take(MAX_TITLE_LEN)
        .collect::<String>();

    (!title.is_empty()).then_some(title)
}

fn fallback_title(content: &str) -> String {
    let mut title = content
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ");
    title = title
        .trim_matches(title_edge_punctuation)
        .chars()
        .take(MAX_TITLE_LEN)
        .collect();
    if title.is_empty() {
        DEFAULT_THREAD_TITLE.to_string()
    } else {
        title
    }
}

fn title_edge_punctuation(c: char) -> bool {
    matches!(c, '"' | '\'' | '.' | ':' | ';' | '!' | '?')
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
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_thread_owner_for_user(&state, owner, thread_id).await?;
    let (content, images) = prepare_message(
        &state,
        thread_id,
        owner,
        request.content,
        request.file_paths,
    )
    .await?;
    let content = normalize_content(content)?;
    let run_id = send_to_runner_with_images(&state, thread_id, content, images).await?;

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
) -> Result<Response, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    // Subscribe to the live topic before reading the snapshot so no event emitted between the two
    // is lost; the snapshot's `last_event_seq` watermark then discards any backlog already covered.
    let events = pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).subscribe();
    let snapshot = state.runner.snapshot(thread_id).await.map_err(pool_error)?;

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, snapshot, events)))
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

pub async fn resolve_approval(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((thread_id, approval_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ApprovalRequest>,
) -> Result<StatusCode, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    state
        .runner
        .resolve_approval(thread_id, approval_id, request.approved)
        .await
        .map_err(pool_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn answer_quiz(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((thread_id, quiz_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<QuizAnswerRequest>,
) -> Result<StatusCode, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    state
        .runner
        .answer_quiz(thread_id, quiz_id, request.answers)
        .await
        .map_err(pool_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn handle_ws(
    mut socket: WebSocket,
    snapshot: ThreadSnapshot,
    mut events: pubsub::Subscriber<AgentEvent>,
) {
    let watermark = snapshot.last_event_seq;
    let snapshot_json = snapshot_event(&snapshot);
    if socket
        .send(Message::Text(snapshot_json.into()))
        .await
        .is_err()
    {
        return;
    }

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
                        // The topic backlog replays events already reflected in the snapshot; the
                        // watermark drops them so the client never applies an event twice.
                        if event.seq <= watermark {
                            continue;
                        }
                        let Ok(data) = serde_json::to_string(&event_response(event)) else {
                            break;
                        };
                        if socket.send(Message::Text(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(pubsub::RecvError::Lagged(_)) => {}
                    Err(pubsub::RecvError::Closed) => break,
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

async fn send_to_runner_with_images(
    state: &ServerState,
    thread_id: Uuid,
    content: String,
    images: Vec<llm::ImageSource>,
) -> Result<RunId, ThreadApiError> {
    state
        .runner
        .send(thread_id, AgentRequest { content, images })
        .await
        .map_err(pool_error)
}

/// True when the default model accepts image inputs.
fn vision_enabled(state: &ServerState) -> bool {
    state
        .model_config
        .model_registry
        .get_or_default(DEFAULT_MODEL)
        .vision
}

/// Splits attachments into images (sent to a vision model) and other files
/// (listed in the message text as before). Image bytes are published via
/// [`crate::api::images::publish_image`]. When the model has no vision support
/// every attachment is treated as a plain file reference.
async fn prepare_message(
    state: &ServerState,
    thread_id: Uuid,
    owner: Uuid,
    content: String,
    file_paths: Vec<String>,
) -> Result<(String, Vec<llm::ImageSource>), ThreadApiError> {
    if file_paths.is_empty() || !vision_enabled(state) {
        return Ok((build_content(content, file_paths), Vec::new()));
    }

    let Some(ref vfs) = state.vfs else {
        return Ok((build_content(content, file_paths), Vec::new()));
    };
    let Ok(workspace_id) = thread_workspace_id(state, thread_id, owner).await else {
        return Ok((build_content(content, file_paths), Vec::new()));
    };

    let fs = crate::vfs::MountedVfs::new(vfs.clone(), workspace_id, owner);
    let public_url = state.config.public_url();

    let mut images = Vec::new();
    let mut other_files = Vec::new();
    for path in file_paths {
        match fs.read_bytes(&path).await {
            Ok((bytes, mime)) if mime.as_deref().is_some_and(|m| m.starts_with("image/")) => {
                match crate::api::images::publish_image(
                    vfs,
                    &state.db,
                    owner,
                    public_url.as_deref(),
                    &bytes,
                    mime.as_deref(),
                )
                .await
                {
                    Ok(image) => images.push(image),
                    Err(error) => {
                        tracing::warn!(%path, %error, "failed to publish attached image");
                        other_files.push(path);
                    }
                }
            }
            _ => other_files.push(path),
        }
    }

    Ok((build_content(content, other_files), images))
}

fn build_content(content: String, file_paths: Vec<String>) -> String {
    if file_paths.is_empty() {
        return content;
    }
    let paths = file_paths
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{content}\n\nAttached files:\n{paths}")
}

fn normalize_content(content: String) -> Result<String, ThreadApiError> {
    let content = content.trim().to_string();
    if content.is_empty() {
        Err(ThreadApiError::BadRequest)
    } else {
        Ok(content)
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
        return ("agent", Some(name.clone()));
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
        AgentPoolError::ApprovalNotFound => ThreadApiError::NotFound,
        AgentPoolError::QuizNotFound => ThreadApiError::NotFound,
        AgentPoolError::AlreadyRunning => ThreadApiError::Conflict,
        AgentPoolError::EventHistoryExpired
        | AgentPoolError::WorkerStopped
        | AgentPoolError::Internal(_) => ThreadApiError::Internal,
    }
}

#[derive(Serialize)]
pub struct UploadedFile {
    name: String,
    path: String,
    size: usize,
}

#[derive(Serialize)]
pub struct UploadResponse {
    files: Vec<UploadedFile>,
}

#[derive(Deserialize)]
pub struct FilesQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateDirectoryRequest {
    path: String,
}

#[derive(Serialize)]
pub struct WorkspaceEntry {
    name: String,
    path: String,
    kind: &'static str,
    size: Option<i64>,
    updated_at: i64,
    mime_type: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceListResponse {
    path: String,
    entries: Vec<WorkspaceEntry>,
}

pub async fn list_files(
    State(state): State<Arc<ServerState>>,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<FilesQuery>,
    headers: HeaderMap,
) -> Result<Json<WorkspaceListResponse>, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let workspace_id = thread_workspace_id(&state, thread_id, owner).await?;
    let path = clean_workspace_path(query.path.as_deref());
    let entries = vfs
        .list(workspace_id, &path)
        .await
        .map_err(|_| ThreadApiError::NotFound)?
        .into_iter()
        .map(|entry| {
            let entry_path = join_workspace_path(&path, &entry.name);
            WorkspaceEntry {
                name: entry.name,
                path: entry_path,
                kind: match entry.kind {
                    crate::vfs::EntryKind::Directory => "directory",
                    crate::vfs::EntryKind::File => "file",
                },
                size: entry.size,
                updated_at: entry.updated_at,
                mime_type: entry.mime_type,
            }
        })
        .collect();

    Ok(Json(WorkspaceListResponse { path, entries }))
}

pub async fn create_directory(
    State(state): State<Arc<ServerState>>,
    Path(thread_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CreateDirectoryRequest>,
) -> Result<StatusCode, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let workspace_id = thread_workspace_id(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&request.path));
    if path.is_empty() {
        return Err(ThreadApiError::BadRequest);
    }

    vfs.create_dir(workspace_id, &path, owner)
        .await
        .map_err(|_| ThreadApiError::BadRequest)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_file(
    State(state): State<Arc<ServerState>>,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<FilesQuery>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::Internal);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let workspace_id = thread_workspace_id(&state, thread_id, owner).await?;
    let directory = clean_workspace_path(query.path.as_deref());

    let mut uploaded = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ThreadApiError::BadRequest)?
    {
        let name = field
            .file_name()
            .filter(|n| !n.is_empty())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "file".to_string());

        let mime_type = field
            .content_type()
            .filter(|ct| !ct.is_empty())
            .map(|ct| ct.to_string());

        let bytes = field
            .bytes()
            .await
            .map_err(|_| ThreadApiError::BadRequest)?;
        let size = bytes.len();

        let path = join_workspace_path(&directory, &name);

        vfs.write_bytes(workspace_id, &path, &bytes, mime_type.as_deref(), owner)
            .await
            .map_err(|_| ThreadApiError::Internal)?;

        // Reference uploaded files under the writable workspace mount so the
        // agent reads them from the workspace, not the read-only global root.
        uploaded.push(UploadedFile {
            path: format!("/~workspace/{path}"),
            name,
            size,
        });
    }

    Ok(Json(UploadResponse { files: uploaded }))
}

pub async fn delete_file(
    State(state): State<Arc<ServerState>>,
    Path((thread_id, path)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let workspace_id = thread_workspace_id(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&path));
    if path.is_empty() {
        return Err(ThreadApiError::BadRequest);
    }

    vfs.delete(workspace_id, &path)
        .await
        .map_err(|_| ThreadApiError::NotFound)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn download_file(
    State(state): State<Arc<ServerState>>,
    Path((thread_id, path)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Response, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let workspace_id = thread_workspace_id(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&path));

    let (bytes, mime_type) = vfs
        .read_bytes(workspace_id, &path)
        .await
        .map_err(|_| ThreadApiError::NotFound)?;

    let content_type = mime_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let filename = path.split('/').next_back().unwrap_or(&path).to_string();

    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(bytes))
        .map_err(|_| ThreadApiError::Internal)
}

async fn thread_workspace_id(
    state: &ServerState,
    thread_id: Uuid,
    owner: Uuid,
) -> Result<Uuid, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let row = state
        .db
        .query_with_params(
            "SELECT project_id FROM threads WHERE id = ? AND owner = ? LIMIT 1",
            vec![Value::Uuid(thread_id), Value::Uuid(owner)],
        )
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    if row.is_empty() {
        return Err(ThreadApiError::NotFound);
    }

    let project_id = row.rows().first().and_then(|r| match r.get("project_id") {
        Some(Value::Uuid(id)) => Some(*id),
        Some(Value::Blob(b)) if b.len() == 16 => Uuid::from_slice(b).ok(),
        Some(Value::Text(s)) => Uuid::parse_str(s).ok(),
        _ => None,
    });

    vfs.get_or_create_workspace(thread_id, project_id, owner)
        .await
        .map_err(|_| ThreadApiError::Internal)
}

fn clean_workspace_path(path: Option<&str>) -> String {
    let raw = path.unwrap_or_default().trim_start_matches('/');
    let path = raw.strip_prefix("~workspace").unwrap_or(raw);

    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .collect::<Vec<_>>()
        .join("/")
}

fn join_workspace_path(parent: &str, name: &str) -> String {
    let name = clean_workspace_path(Some(name));
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
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
            AgentEventKind::ApprovalResolved {
                approval_id,
                approved,
            } => EventKindResponse::ApprovalResolved {
                approval_id: approval_id.to_string(),
                approved,
            },
            AgentEventKind::WaitingForQuiz { quiz_id, questions } => {
                EventKindResponse::WaitingForQuiz {
                    quiz_id: quiz_id.to_string(),
                    questions: quiz_questions_response(questions),
                }
            }
            AgentEventKind::QuizAnswered { quiz_id } => EventKindResponse::QuizAnswered {
                quiz_id: quiz_id.to_string(),
            },
            AgentEventKind::RunFinished => EventKindResponse::RunFinished,
            AgentEventKind::RunFailed { error } => EventKindResponse::RunFailed { error },
            AgentEventKind::RunCancelled => EventKindResponse::RunCancelled,
        },
    }
}

fn snapshot_event(snapshot: &ThreadSnapshot) -> String {
    let event = EventResponse {
        seq: snapshot.last_event_seq,
        thread_id: snapshot.thread_id.to_string(),
        run_id: None,
        kind: EventKindResponse::Snapshot {
            status: match snapshot.status {
                ThreadStatus::Idle => "idle",
                ThreadStatus::Running { .. } => "running",
            },
            in_progress: snapshot
                .in_progress
                .as_ref()
                .map(|message| SnapshotMessageResponse {
                    run_id: message.run_id.0.to_string(),
                    content: message.content.clone(),
                    thinking: message.thinking.clone(),
                }),
            pending_approval: snapshot
                .pending_approval
                .as_ref()
                .map(|approval| ApprovalResponse {
                    approval_id: approval.approval_id.to_string(),
                    message: approval.message.clone(),
                }),
            pending_quiz: snapshot.pending_quiz.as_ref().map(|quiz| QuizResponse {
                quiz_id: quiz.quiz_id.to_string(),
                questions: quiz_questions_response(quiz.questions.clone()),
            }),
        },
    };

    serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())
}

fn quiz_questions_response(
    questions: Vec<friday_agent::QuizQuestion>,
) -> Vec<QuizQuestionResponse> {
    questions
        .into_iter()
        .map(|question| QuizQuestionResponse {
            question: question.question,
            options: question.options,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn title_update_persists_and_is_read_back() {
        use crate::db::users;
        use minisql::ConnectionPool;

        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(crate::db::get_migrations())
            .await
            .unwrap();

        let owner = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username("u")
            .password_hash("x")
            .personality(Option::<&str>::None)
            .execute(&db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(owner)
            .title(DEFAULT_THREAD_TITLE)
            .execute(&db)
            .await
            .unwrap();

        threads::update()
            .title("Generated title")
            .where_(threads::id.eq(thread_id))
            .execute(&db)
            .await
            .unwrap();

        let rows = db
            .query_with_params(
                "SELECT title FROM threads WHERE id = ?",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .unwrap();
        assert_eq!(
            rows.rows()[0].get_text("title"),
            Some("Generated title"),
            "title update did not persist"
        );
    }

    #[test]
    fn uuid_value_accepts_sqlite_blob_uuid() {
        let id = Uuid::now_v7();
        let value = Value::Blob(id.as_bytes().to_vec());

        assert_eq!(uuid_value(Some(&value)).unwrap(), id);
    }

    #[test]
    fn build_content_appends_file_paths() {
        let result = build_content(
            "hello".to_string(),
            vec![
                "/~workspace/a.txt".to_string(),
                "/~workspace/b.pdf".to_string(),
            ],
        );
        assert_eq!(
            result,
            "hello\n\nAttached files:\n- /~workspace/a.txt\n- /~workspace/b.pdf"
        );
    }

    #[test]
    fn build_content_no_files_returns_original() {
        let result = build_content("hello".to_string(), vec![]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn clean_workspace_path_accepts_legacy_workspace_prefix() {
        assert_eq!(
            clean_workspace_path(Some("/~workspace/reports/a.pdf")),
            "reports/a.pdf"
        );
        assert_eq!(
            clean_workspace_path(Some("/reports/a.pdf")),
            "reports/a.pdf"
        );
    }
}
