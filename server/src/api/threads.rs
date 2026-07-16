use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{
        Multipart, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use minisql::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use llm::{CompletionRequest, Message as LlmMessage, Role as LlmRole};
use stride_agent::{DEFAULT_MODEL, TokenUsage};

use crate::{
    ServerState,
    api::{
        auth::{self, AuthError},
        projects::ProjectResponse,
    },
    db::{
        MessageFormat, Role, messages, projects, thread_agents, thread_events, threads, user_models,
    },
    model_registry,
    runner::{
        AgentEvent, AgentEventKind, AgentPoolError, AgentRequest, RunId, ThreadSnapshot,
        ThreadStatus, thread_events_topic,
    },
    user_events::{self, UserEventKind},
};

/// Placeholder title a new thread gets until the LLM generates a real one.
pub(crate) const DEFAULT_THREAD_TITLE: &str = "New chat";
/// Telegram caps forum topic names at 128 characters; we cap every generated title to match.
const MAX_TITLE_LEN: usize = 128;
const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(20);
const TITLE_GENERATOR_MODEL: &str = "title_generator";

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
    created_at: i64,
    role: &'static str,
    format: &'static str,
    content: String,
    thinking: Option<String>,
    tool_call_name: Option<String>,
    tool_call_id: Option<String>,
    tool_calls: Vec<ToolCallResponse>,
}

#[derive(Clone, Serialize)]
pub struct ToolCallResponse {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize)]
pub struct AgentResponse {
    pub agent_id: String,
    pub agent_path: String,
    pub parent_tool_call_id: Option<String>,
    pub name: String,
    pub model: String,
    pub result: Option<String>,
    pub finished: bool,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct ThreadPageData {
    pub username: String,
    pub full_name: String,
    pub thread_id: String,
    pub current_title: String,
    pub selected_model: String,
    pub models: Vec<model_registry::ModelSummary>,
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
}

#[derive(Serialize)]
pub struct MessageTemplateData {
    pub id: String,
    pub seq: u64,
    pub created_at: i64,
    pub role: &'static str,
    pub format: &'static str,
    pub message_type: &'static str,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<ToolCallResponse>,
    pub content: String,
    pub thinking: Option<String>,
    pub has_thinking: bool,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    content: String,
    project_id: Option<Uuid>,
    model: Option<String>,
    #[serde(default)]
    file_paths: Vec<String>,
    /// Ids of files uploaded to the staging area before this thread existed.
    /// They are moved into the thread's workspace before the run starts.
    #[serde(default)]
    staged_uploads: Vec<Uuid>,
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
struct EventResponse<K> {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    seq: u64,
    thread_id: String,
    run_id: Option<String>,
    agent_path: Vec<String>,
    kind: K,
}

#[derive(Serialize)]
struct SnapshotMessageResponse {
    message_id: String,
    run_id: String,
    content: String,
    format: &'static str,
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
#[serde(tag = "type", rename_all = "snake_case")]
enum SnapshotKind {
    Snapshot {
        status: &'static str,
        in_progress: Option<SnapshotMessageResponse>,
        pending_approvals: Vec<ApprovalResponse>,
        pending_quizzes: Vec<QuizResponse>,
    },
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

#[derive(Deserialize)]
pub struct RenameThreadRequest {
    title: String,
}

#[derive(Deserialize)]
pub struct UpdateThreadModelRequest {
    model: Option<String>,
}

#[derive(Serialize)]
pub struct ArchivedThreadResponse {
    id: String,
    title: String,
    project_id: Option<String>,
    archived_at: i64,
    last_activity_at: i64,
}

/// Renames a thread. Mirrors `projects::rename` — trims the title, rejects an
/// empty one, and asserts ownership.
pub async fn rename_thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Json(request): Json<RenameThreadRequest>,
) -> Result<Json<ThreadResponse>, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_thread_owner_for_user(&state, owner, thread_id).await?;

    let title = request.title.trim().to_string();
    if title.is_empty() {
        return Err(ThreadApiError::BadRequest);
    }

    threads::update()
        .title(title.as_str())
        .where_(threads::id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    user_events::publish(
        owner,
        state.id_gen.new_uuid_v7(),
        UserEventKind::ThreadRenamed {
            thread_id,
            title: title.clone(),
        },
    );

    let project_id = threads::select_cols((threads::project_id,))
        .where_(threads::id.eq(thread_id))
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?
        .into_iter()
        .next()
        .and_then(|(pid,)| pid)
        .map(|id| id.to_string());

    Ok(Json(ThreadResponse {
        id: thread_id.to_string(),
        title,
        project_id,
    }))
}

pub async fn update_thread_model(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Json(request): Json<UpdateThreadModelRequest>,
) -> Result<StatusCode, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_thread_owner_for_user(&state, owner, thread_id).await?;

    let model = validate_chat_model(&state, owner, request.model.as_deref()).await?;
    threads::update()
        .last_model(model.as_deref())
        .where_(threads::id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Archives a thread: it disappears from the sidebar but keeps all messages and
/// files. Recorded with the archival timestamp that drives auto-removal.
pub async fn archive_thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<StatusCode, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_thread_owner_for_user(&state, owner, thread_id).await?;

    threads::update()
        .archived_at(Some(state.clock.now_unix_millis()))
        .where_(threads::id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    user_events::publish(
        owner,
        state.id_gen.new_uuid_v7(),
        UserEventKind::ThreadArchived { thread_id },
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Restores an archived thread and resets its last-activity to now, so the
/// auto-archive clock starts fresh from the unarchival date.
pub async fn unarchive_thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<StatusCode, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_thread_owner_for_user(&state, owner, thread_id).await?;

    threads::update()
        .archived_at(Option::<i64>::None)
        .last_activity_at(Some(state.clock.now_unix_millis()))
        .where_(threads::id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    user_events::publish(
        owner,
        state.id_gen.new_uuid_v7(),
        UserEventKind::ThreadRestored { thread_id },
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Permanently deletes a thread: messages, Telegram links, the standalone
/// workspace (files + version history + blobs), then the thread row itself.
pub async fn delete_thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<StatusCode, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_thread_owner_for_user(&state, owner, thread_id).await?;

    hard_delete_thread(&state, thread_id).await?;

    user_events::publish(
        owner,
        state.id_gen.new_uuid_v7(),
        UserEventKind::ThreadDeleted { thread_id },
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Shared teardown used by the delete endpoint and the retention sweeper.
/// Cancels any in-flight run first so the agent cannot resurrect rows mid-delete.
pub(crate) async fn hard_delete_thread(
    state: &ServerState,
    thread_id: Uuid,
) -> Result<(), ThreadApiError> {
    let _ = state.runner.cancel_run(thread_id).await;

    messages::delete()
        .where_(messages::parent_thread.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    thread_events::delete()
        .where_(thread_events::thread_id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    thread_agents::delete()
        .where_(thread_agents::thread_id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    // Telegram bridge rows carry unique foreign keys onto the thread; drop them
    // before the thread row. Best-effort: absent tables/rows are not an error.
    let _ = state
        .db
        .query_with_params(
            "DELETE FROM telegram_message_links WHERE thread_id = ?",
            vec![Value::Uuid(thread_id)],
        )
        .await;
    let _ = state
        .db
        .query_with_params(
            "DELETE FROM telegram_threads WHERE thread_id = ?",
            vec![Value::Uuid(thread_id)],
        )
        .await;

    if let Some(vfs) = &state.vfs
        && let Err(error) = vfs.delete_thread_workspace(thread_id).await
    {
        tracing::warn!(%thread_id, %error, "failed to delete thread workspace");
    }

    threads::delete()
        .where_(threads::id.eq(thread_id))
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    Ok(())
}

/// Lists the owner's archived threads, newest archival first, with the
/// timestamps the archive page renders.
pub async fn list_archived_threads(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ArchivedThreadResponse>>, ThreadApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;

    let result = state
        .db
        .query_with_params(
            "SELECT id, title, project_id, archived_at, last_activity_at FROM threads \
             WHERE owner = ? AND archived_at IS NOT NULL ORDER BY archived_at DESC",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    let mut threads = Vec::new();
    for row in result.rows() {
        let id = uuid_value(row.get("id"))?;
        let project_id = match row.get("project_id") {
            Some(Value::Uuid(id)) => Some(id.to_string()),
            Some(Value::Blob(bytes)) if bytes.len() == 16 => {
                Uuid::from_slice(bytes).ok().map(|id| id.to_string())
            }
            _ => None,
        };
        threads.push(ArchivedThreadResponse {
            title: row.get_text("title").unwrap_or("Untitled").to_string(),
            project_id,
            archived_at: row.get_int("archived_at").unwrap_or_else(|| uuid_v7_ms(id)),
            last_activity_at: row
                .get_int("last_activity_at")
                .unwrap_or_else(|| uuid_v7_ms(id)),
            id: id.to_string(),
        });
    }

    Ok(Json(threads))
}

pub async fn thread_page_data(
    state: &ServerState,
    headers: &HeaderMap,
    thread_id: Option<Uuid>,
) -> Result<ThreadPageData, ThreadApiError> {
    let owner = auth::authenticated_user(state, headers).await?;
    let profile = crate::api::personal::load(state, owner)
        .await
        .map_err(|_| ThreadApiError::Internal)?;
    let all_threads = thread_summaries(state, owner).await?;
    let all_projects = project_summaries(state, owner).await?;
    let models = model_registry::list_available_models(&state.config, &state.db, owner)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    let current_title = thread_id
        .and_then(|id| {
            all_threads
                .iter()
                .find(|thread| thread.id == id.to_string())
                .map(|thread| thread.title.clone())
        })
        .unwrap_or_else(|| "New thread".to_string());

    let (messages, running, selected_model) = if let Some(thread_id) = thread_id {
        require_thread_owner_for_user(state, owner, thread_id).await?;
        (
            thread_messages(state, thread_id).await?,
            matches!(
                state.runner.status(thread_id).await.map_err(pool_error)?,
                ThreadStatus::Running { .. }
            ),
            thread_selected_model(state, thread_id).await?,
        )
    } else {
        (Vec::new(), false, String::new())
    };

    let thread_template_data: Vec<ThreadTemplateData> = all_threads
        .iter()
        .map(|thread| ThreadTemplateData {
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
        username: profile.username,
        full_name: profile.full_name,
        thread_id: thread_id.map(|id| id.to_string()).unwrap_or_default(),
        current_title,
        selected_model,
        models,
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
                    created_at: message.created_at,
                    role: message.role,
                    format: message.format,
                    message_type,
                    tool_name,
                    tool_call_id: message.tool_call_id,
                    tool_calls: message.tool_calls,
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
            "SELECT id, title, project_id FROM threads WHERE owner = ? AND archived_at IS NULL ORDER BY id DESC",
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

async fn thread_selected_model(
    state: &ServerState,
    thread_id: Uuid,
) -> Result<String, ThreadApiError> {
    Ok(threads::select_cols((threads::last_model,))
        .where_(threads::id.eq(thread_id))
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?
        .into_iter()
        .next()
        .and_then(|(model,)| model)
        .unwrap_or_default())
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
    let model = validate_chat_model(&state, owner, request.model.as_deref()).await?;
    let project_id = request.project_id;
    let thread_id = state.id_gen.new_uuid_v7();

    let mut insert = threads::insert()
        .id(thread_id)
        .owner(owner)
        .title(DEFAULT_THREAD_TITLE)
        .last_activity_at(Some(state.clock.now_unix_millis()));
    if let Some(pid) = project_id {
        insert = insert.project_id(pid);
    }
    insert
        .execute(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    user_events::publish(
        owner,
        state.id_gen.new_uuid_v7(),
        UserEventKind::ThreadCreated {
            thread_id,
            title: DEFAULT_THREAD_TITLE.to_string(),
            project_id,
        },
    );

    let mut file_paths = request.file_paths;
    let staged =
        materialize_staged_uploads(&state, thread_id, owner, &request.staged_uploads).await?;
    file_paths.extend(staged);

    let (content, images) = prepare_message(
        &state,
        thread_id,
        owner,
        request.content,
        file_paths,
        model.as_deref(),
    )
    .await?;
    let content = normalize_content(content)?;

    let run_id =
        send_to_runner_with_images(&state, thread_id, content.clone(), images, model).await?;

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
                let stored = threads::select_cols((threads::title, threads::owner))
                    .where_(threads::id.eq(thread_id))
                    .all(&state.db)
                    .await
                    .ok()
                    .and_then(|rows| rows.into_iter().next());
                match stored {
                    Some((stored, owner)) if stored == title => user_events::publish(
                        owner,
                        state.id_gen.new_uuid_v7(),
                        UserEventKind::ThreadRenamed {
                            thread_id,
                            title: title.clone(),
                        },
                    ),
                    Some((stored, _)) => tracing::warn!(
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
    config: &Arc<stride_agent::AgentConfig>,
    content: &str,
) -> Result<(String, bool), llm::Error> {
    let model_key = if config.model_registry.get(TITLE_GENERATOR_MODEL).is_some() {
        TITLE_GENERATOR_MODEL
    } else {
        DEFAULT_MODEL
    };
    let model = config.model_registry.get_or_default(model_key);
    let provider = config
        .model_registry
        .provider(model_key)
        .unwrap_or("unknown")
        .to_string();
    let request = title_generation_request(&model.model_name, content);

    let completion = model.api.get_completion(&model.token, request).await?;
    config.usage_observer.token_usage(TokenUsage {
        input_tokens: completion.usage.prompt_tokens as u64,
        output_tokens: completion.usage.completion_tokens as u64,
        model: model_key.to_string(),
        provider,
    });

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
    .max_tokens(4096)
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

/// Lists every subagent spawned in a thread, most recent first. Drives the
/// Subagents side panel.
pub async fn list_agents(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<Json<Vec<AgentResponse>>, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;

    let rows = thread_agents::select()
        .where_(thread_agents::thread_id.eq(thread_id))
        .order_by_desc(thread_agents::created_at)
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|row| AgentResponse {
                agent_id: row.agent_id.to_string(),
                agent_path: row.agent_path,
                parent_tool_call_id: row.parent_tool_call_id,
                name: row.name,
                model: row.model,
                result: row.result,
                finished: row.finished,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

/// Returns one subagent's transcript: every message whose `agent_path` is the
/// agent's path or a descendant of it (so nested grandchildren are included).
pub async fn agent_messages(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((thread_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<MessageResponse>>, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;

    let agent = thread_agents::select()
        .where_(
            thread_agents::agent_id
                .eq(agent_id)
                .and(thread_agents::thread_id.eq(thread_id)),
        )
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?
        .into_iter()
        .next()
        .ok_or(ThreadApiError::NotFound)?;

    let path = agent.agent_path;
    let descendant_prefix = format!("{path}/");
    let messages = messages_where(&state, thread_id, |candidate| {
        matches!(candidate, Some(candidate)
            if candidate == path || candidate.starts_with(&descendant_prefix))
    })
    .await?;
    Ok(Json(messages))
}

async fn thread_messages(
    state: &ServerState,
    thread_id: Uuid,
) -> Result<Vec<MessageResponse>, ThreadApiError> {
    // Root-only: subagent messages carry a non-null `agent_path` and are served
    // by the Subagents endpoints, never mixed into the main chat.
    messages_where(state, thread_id, |path| path.is_none()).await
}

/// Loads a thread's messages, keeps the ones whose `agent_path` satisfies
/// `keep`, and maps them to the shared [`MessageResponse`] shape.
async fn messages_where(
    state: &ServerState,
    thread_id: Uuid,
    keep: impl Fn(Option<&str>) -> bool,
) -> Result<Vec<MessageResponse>, ThreadApiError> {
    let rows = messages::select()
        .where_(messages::parent_thread.eq(thread_id))
        .order_by_asc(messages::seq)
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    Ok(rows
        .into_iter()
        .filter(|row| keep(row.agent_path.as_deref()))
        .map(|row| {
            let tool_calls = tool_calls(row.tool_calls.as_deref());
            let tool_call_name = tool_calls.first().map(|call| call.name.clone());
            MessageResponse {
                id: row.id.to_string(),
                seq: row.seq,
                created_at: uuid_v7_ms(row.id),
                role: role_name(row.role),
                format: message_format_name(row.content_format.unwrap_or(MessageFormat::Markdown)),
                content: row.content,
                thinking: row.thinking,
                tool_call_name,
                tool_call_id: row.tool_call_id,
                tool_calls,
            }
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
    let model = effective_request_model(&state, owner, thread_id, request.model.as_deref()).await?;

    let mut file_paths = request.file_paths;
    let staged =
        materialize_staged_uploads(&state, thread_id, owner, &request.staged_uploads).await?;
    file_paths.extend(staged);

    let (content, images) = prepare_message(
        &state,
        thread_id,
        owner,
        request.content,
        file_paths,
        model.as_deref(),
    )
    .await?;
    let content = normalize_content(content)?;
    let run_id = send_to_runner_with_images(&state, thread_id, content, images, model).await?;

    Ok(Json(SendMessageResponse {
        thread_id: thread_id.to_string(),
        run_id: run_id.0.to_string(),
    }))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    /// Client's last applied event seq. When present, the handler replays
    /// journaled events with `seq > after` before the snapshot so a reconnecting
    /// client recovers everything it missed.
    after: Option<u64>,
}

pub async fn events(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<EventsQuery>,
) -> Result<Response, ThreadApiError> {
    require_thread_owner(&state, &headers, thread_id).await?;
    // Subscribe to the live topic before reading the snapshot so no event emitted between the two
    // is lost; the snapshot's `last_event_seq` watermark then discards any backlog already covered.
    let events = pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).subscribe();

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, thread_id, query.after, events)))
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
    state: Arc<ServerState>,
    thread_id: Uuid,
    after: Option<u64>,
    mut events: pubsub::Subscriber<AgentEvent>,
) {
    // `last_sent` is the highest seq the client has been sent; live events at or
    // below it are dropped as already covered by replay or snapshot.
    let mut last_sent = after.unwrap_or(0);

    // Only replay the journal when the client supplied a cursor: a fresh open has
    // already loaded history over REST and just needs snapshot + live tail.
    if after.is_some()
        && !replay_journal(&mut socket, &state, thread_id, last_sent, &mut last_sent).await
    {
        return;
    }
    if !send_snapshot(&mut socket, &state, thread_id, &mut last_sent).await {
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
                        if event.seq <= last_sent {
                            continue;
                        }
                        last_sent = event.seq;
                        let Ok(data) = serde_json::to_string(&event_response(event)) else {
                            break;
                        };
                        if socket.send(Message::Text(data.into())).await.is_err() {
                            break;
                        }
                    }
                    // A slow consumer that overflowed the ring recovers from the durable journal
                    // plus a fresh snapshot instead of silently dropping the skipped events.
                    Err(pubsub::RecvError::Lagged(_)) => {
                        if !replay_journal(&mut socket, &state, thread_id, last_sent, &mut last_sent).await {
                            break;
                        }
                        if !send_snapshot(&mut socket, &state, thread_id, &mut last_sent).await {
                            break;
                        }
                    }
                    Err(pubsub::RecvError::Decode(_)) => {}
                    Err(pubsub::RecvError::Closed) => break,
                }
            }
        }
    }
}

/// Streams journaled events with `seq > from`, advancing `last_sent`. Returns
/// `false` if the socket closed mid-send.
async fn replay_journal(
    socket: &mut WebSocket,
    state: &ServerState,
    thread_id: Uuid,
    from: u64,
    last_sent: &mut u64,
) -> bool {
    for event in crate::runner::inproc::journal_events_after(&state.db, thread_id, from).await {
        let seq = event.seq;
        let Ok(data) = serde_json::to_string(&event_response(event)) else {
            continue;
        };
        if socket.send(Message::Text(data.into())).await.is_err() {
            return false;
        }
        *last_sent = (*last_sent).max(seq);
    }
    true
}

/// Sends a fresh snapshot frame and lifts `last_sent` to its watermark. Returns
/// `false` if the snapshot is unavailable or the socket closed.
async fn send_snapshot(
    socket: &mut WebSocket,
    state: &ServerState,
    thread_id: Uuid,
    last_sent: &mut u64,
) -> bool {
    let Ok(snapshot) = state.runner.snapshot(thread_id).await else {
        return false;
    };
    let json = snapshot_event(&snapshot);
    if socket.send(Message::Text(json.into())).await.is_err() {
        return false;
    }
    *last_sent = (*last_sent).max(snapshot.last_event_seq);
    true
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
    model: Option<String>,
) -> Result<RunId, ThreadApiError> {
    state
        .runner
        .send(
            thread_id,
            AgentRequest {
                content,
                images,
                model,
            },
        )
        .await
        .map_err(pool_error)
}

async fn validate_chat_model(
    state: &ServerState,
    owner: Uuid,
    model: Option<&str>,
) -> Result<Option<String>, ThreadApiError> {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let available = model_registry::list_available_models(&state.config, &state.db, owner)
        .await
        .map_err(|_| ThreadApiError::Internal)?;
    if available
        .iter()
        .any(|available| available.key.as_str() == model)
    {
        Ok(Some(model.to_string()))
    } else {
        Err(ThreadApiError::BadRequest)
    }
}

async fn effective_request_model(
    state: &ServerState,
    owner: Uuid,
    thread_id: Uuid,
    requested: Option<&str>,
) -> Result<Option<String>, ThreadApiError> {
    if requested
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        return validate_chat_model(state, owner, requested).await;
    }

    let saved = thread_selected_model(state, thread_id).await?;
    if saved.is_empty() {
        return Ok(None);
    }
    let available = model_registry::list_available_models(&state.config, &state.db, owner)
        .await
        .map_err(|_| ThreadApiError::Internal)?;
    Ok(available
        .iter()
        .any(|model| model.key.as_str() == saved)
        .then_some(saved))
}

fn vision_enabled(state: &ServerState, model: Option<&str>) -> bool {
    let key = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    state.model_config.model_registry.get_or_default(key).vision
}

async fn model_has_vision(
    state: &ServerState,
    owner: Uuid,
    model: Option<&str>,
) -> Result<bool, ThreadApiError> {
    let key = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    if state.model_config.model_registry.get(key).is_some() {
        return Ok(vision_enabled(state, Some(key)));
    }
    let rows = user_models::select_cols((user_models::vision,))
        .where_(user_models::owner.eq(owner).and(user_models::name.eq(key)))
        .all(&state.db)
        .await
        .map_err(|_| ThreadApiError::Internal)?;
    Ok(rows
        .into_iter()
        .next()
        .map(|(vision,)| vision)
        .unwrap_or(false))
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
    model: Option<&str>,
) -> Result<(String, Vec<llm::ImageSource>), ThreadApiError> {
    if file_paths.is_empty() || !model_has_vision(state, owner, model).await? {
        return Ok((build_content(content, file_paths), Vec::new()));
    }

    let Some(ref vfs) = state.vfs else {
        return Ok((build_content(content, file_paths), Vec::new()));
    };
    let Ok(area) = thread_writable_area(state, thread_id, owner).await else {
        return Ok((build_content(content, file_paths), Vec::new()));
    };

    let fs = crate::vfs::MountedVfs::from_writable_area(vfs.clone(), owner, area);
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

fn message_format_name(format: MessageFormat) -> &'static str {
    match format {
        MessageFormat::Markdown => "markdown",
        MessageFormat::Html => "html",
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

fn tool_calls(tool_calls: Option<&str>) -> Vec<ToolCallResponse> {
    let Some(tool_calls) = tool_calls else {
        return Vec::new();
    };
    let Ok(calls) = serde_json::from_str::<Vec<llm::ToolCallChunk>>(tool_calls) else {
        return Vec::new();
    };
    calls
        .into_iter()
        .filter_map(|call| {
            let function = call.function?;
            Some(ToolCallResponse {
                id: call.id?,
                name: function.name?,
                arguments: function.arguments.unwrap_or_default(),
            })
        })
        .collect()
}

/// The millisecond timestamp packed into a UUIDv7's leading 48 bits. Used as a
/// last-activity/created fallback for threads that predate the timestamp columns.
pub(crate) fn uuid_v7_ms(id: Uuid) -> i64 {
    let bytes = id.as_bytes();
    let mut ts: u64 = 0;
    for byte in &bytes[0..6] {
        ts = (ts << 8) | u64::from(*byte);
    }
    ts as i64
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
pub struct DownloadQuery {
    version: Option<i64>,
}

#[derive(Deserialize)]
pub struct VersionsQuery {
    path: String,
}

#[derive(Deserialize)]
pub struct CreateDirectoryRequest {
    path: String,
}

#[derive(Deserialize)]
pub struct RestoreVersionRequest {
    path: String,
    version: i64,
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

#[derive(Serialize)]
pub struct FileVersionResponse {
    version: i64,
    size: i64,
    created_at: i64,
    mime_type: Option<String>,
}

#[derive(Serialize)]
pub struct FileVersionsResponse {
    path: String,
    versions: Vec<FileVersionResponse>,
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
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let path = clean_workspace_path(query.path.as_deref());
    let entries = vfs
        .area_list(&area, owner, &path)
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

pub async fn list_file_versions(
    State(state): State<Arc<ServerState>>,
    Path(thread_id): Path<Uuid>,
    Query(query): Query<VersionsQuery>,
    headers: HeaderMap,
) -> Result<Json<FileVersionsResponse>, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&query.path));
    if path.is_empty() {
        return Err(ThreadApiError::BadRequest);
    }

    let versions = vfs
        .area_list_versions(&area, owner, &path)
        .await
        .map_err(|_| ThreadApiError::NotFound)?
        .into_iter()
        .map(|version| FileVersionResponse {
            version: version.version,
            size: version.size,
            created_at: version.created_at,
            mime_type: version.mime_type,
        })
        .collect();

    Ok(Json(FileVersionsResponse { path, versions }))
}

pub async fn restore_file_version(
    State(state): State<Arc<ServerState>>,
    Path(thread_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<RestoreVersionRequest>,
) -> Result<StatusCode, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&request.path));
    if path.is_empty() || request.version < 0 {
        return Err(ThreadApiError::BadRequest);
    }

    vfs.area_restore_version(&area, owner, &path, request.version)
        .await
        .map_err(|_| ThreadApiError::NotFound)?;

    Ok(StatusCode::NO_CONTENT)
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
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&request.path));
    if path.is_empty() {
        return Err(ThreadApiError::BadRequest);
    }

    vfs.area_create_dir(&area, owner, &path)
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
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let root = writable_root_path(&area);
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

        vfs.area_write_bytes(&area, owner, &path, &bytes, mime_type.as_deref())
            .await
            .map_err(|_| ThreadApiError::Internal)?;

        // Reference uploaded files under the thread's writable root so the agent
        // reads them from there, not the read-only global root.
        uploaded.push(UploadedFile {
            path: format!("{root}/{path}"),
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
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&path));
    if path.is_empty() {
        return Err(ThreadApiError::BadRequest);
    }

    vfs.area_delete(&area, owner, &path)
        .await
        .map_err(|_| ThreadApiError::NotFound)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn download_file(
    State(state): State<Arc<ServerState>>,
    Path((thread_id, path)): Path<(Uuid, String)>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<Response, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let owner = auth::authenticated_user(&state, &headers).await?;
    let area = thread_writable_area(&state, thread_id, owner).await?;
    let path = clean_workspace_path(Some(&path));

    let (bytes, mime_type) = if let Some(version) = query.version {
        if version < 0 {
            return Err(ThreadApiError::BadRequest);
        }
        vfs.area_read_version(&area, owner, &path, version)
            .await
            .map_err(|_| ThreadApiError::NotFound)?
    } else {
        vfs.area_read_bytes(&area, owner, &path)
            .await
            .map_err(|_| ThreadApiError::NotFound)?
    };

    super::file_response(&path, bytes, mime_type).map_err(|_| ThreadApiError::Internal)
}

/// Moves the owner's staged uploads into a thread's writable area under
/// `uploads/`, returning the agent-facing absolute paths. Unknown or non-owned
/// ids are skipped so a stale reference never fails the whole message.
async fn materialize_staged_uploads(
    state: &ServerState,
    thread_id: Uuid,
    owner: Uuid,
    staged_ids: &[Uuid],
) -> Result<Vec<String>, ThreadApiError> {
    if staged_ids.is_empty() {
        return Ok(Vec::new());
    }
    let Some(ref vfs) = state.vfs else {
        return Ok(Vec::new());
    };

    let area = thread_writable_area(state, thread_id, owner).await?;
    let root = writable_root_path(&area);

    let mut paths = Vec::new();
    for &id in staged_ids {
        let staged = match vfs.take_staged_upload(owner, id).await {
            Ok(staged) => staged,
            Err(error) => {
                tracing::warn!(%thread_id, %id, %error, "skipping unknown staged upload");
                continue;
            }
        };
        let rel = join_workspace_path("uploads", &staged.name);
        vfs.area_write_bytes(
            &area,
            owner,
            &rel,
            &staged.bytes,
            staged.mime_type.as_deref(),
        )
        .await
        .map_err(|_| ThreadApiError::Internal)?;
        paths.push(format!("{root}/{rel}"));
    }
    Ok(paths)
}

/// Resolves a thread's writable area: a project thread writes into the
/// project's folder in the owner's global files; an ungrouped thread keeps its
/// own standalone workspace.
async fn thread_writable_area(
    state: &ServerState,
    thread_id: Uuid,
    owner: Uuid,
) -> Result<crate::vfs::WritableArea, ThreadApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(ThreadApiError::NotFound);
    };

    let row = state
        .db
        .query_with_params(
            "SELECT t.project_id AS project_id, p.title AS title \
             FROM threads t LEFT JOIN projects p ON p.id = t.project_id \
             WHERE t.id = ? AND t.owner = ? LIMIT 1",
            vec![Value::Uuid(thread_id), Value::Uuid(owner)],
        )
        .await
        .map_err(|_| ThreadApiError::Internal)?;

    if row.is_empty() {
        return Err(ThreadApiError::NotFound);
    }

    let title = row
        .rows()
        .first()
        .and_then(|r| r.get_text("title"))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    if let Some(title) = title {
        let prefix = vfs
            .ensure_project_dir(owner, &title)
            .await
            .map_err(|_| ThreadApiError::Internal)?;
        return Ok(crate::vfs::WritableArea::ProjectDir(prefix));
    }

    let workspace_id = vfs
        .get_or_create_workspace(thread_id, None, owner)
        .await
        .map_err(|_| ThreadApiError::Internal)?;
    Ok(crate::vfs::WritableArea::Workspace(workspace_id))
}

/// The absolute path the agent and download URLs use for a thread's writable
/// directory.
fn writable_root_path(area: &crate::vfs::WritableArea) -> String {
    match area {
        crate::vfs::WritableArea::Workspace(_) => crate::vfs::AGENT_HOME.to_string(),
        crate::vfs::WritableArea::ProjectDir(prefix) => {
            format!("{}/{prefix}", crate::vfs::USER_HOME)
        }
    }
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

fn event_response(event: AgentEvent) -> EventResponse<AgentEventKind> {
    EventResponse {
        id: Some(event.id.to_string()),
        seq: event.seq,
        thread_id: event.thread_id.to_string(),
        run_id: event.run_id.map(|run_id| run_id.0.to_string()),
        agent_path: event
            .agent_path
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        kind: event.kind,
    }
}

fn snapshot_event(snapshot: &ThreadSnapshot) -> String {
    let event = EventResponse {
        id: None,
        seq: snapshot.last_event_seq,
        thread_id: snapshot.thread_id.to_string(),
        run_id: None,
        agent_path: Vec::new(),
        kind: SnapshotKind::Snapshot {
            status: match snapshot.status {
                ThreadStatus::Idle => "idle",
                ThreadStatus::Running { .. } => "running",
            },
            in_progress: snapshot
                .in_progress
                .as_ref()
                .map(|message| SnapshotMessageResponse {
                    message_id: message.message_id.to_string(),
                    run_id: message.run_id.0.to_string(),
                    content: message.content.clone(),
                    format: message_format_name(message.format),
                    thinking: message.thinking.clone(),
                }),
            pending_approvals: snapshot
                .pending_approvals
                .iter()
                .map(|approval| ApprovalResponse {
                    approval_id: approval.approval_id.to_string(),
                    message: approval.message.clone(),
                })
                .collect(),
            pending_quizzes: snapshot
                .pending_quizzes
                .iter()
                .map(|quiz| QuizResponse {
                    quiz_id: quiz.quiz_id.to_string(),
                    questions: quiz_questions_response(quiz.questions.clone()),
                })
                .collect(),
        },
    };

    serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())
}

fn quiz_questions_response(
    questions: Vec<stride_agent::QuizQuestion>,
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

    #[test]
    fn event_response_keeps_shared_attachment_ids() {
        let event_id = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        let run_id = RunId(Uuid::now_v7());
        let message_id = Uuid::now_v7();
        let agent_id = Uuid::now_v7();
        let response = event_response(AgentEvent {
            id: event_id,
            seq: 7,
            thread_id,
            run_id: Some(run_id),
            agent_path: vec![agent_id],
            kind: AgentEventKind::TextDelta {
                message_id,
                delta: "hello".to_owned(),
            },
        });
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["id"], event_id.to_string());
        assert_eq!(json["agent_path"][0], agent_id.to_string());
        assert_eq!(json["kind"]["type"], "text_delta");
        assert_eq!(json["kind"]["message_id"], message_id.to_string());
    }

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

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    #[test]
    fn uuid_v7_ms_recovers_generation_time() {
        let before = now_ms();
        let id = Uuid::now_v7();
        let after = now_ms();
        let ts = uuid_v7_ms(id);
        assert!(
            ts >= before - 5 && ts <= after + 5,
            "ts={ts} before={before} after={after}"
        );
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
