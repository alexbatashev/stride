use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use hmac::{Hmac, Mac};
use http_body_util::Full;
use hyper::Request;
use minisql::ConnectionPool;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stride_agent::QuizQuestion;
use tokio::time::timeout;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    api::threads::DEFAULT_THREAD_TITLE,
    db::{
        Role, messages, projects, telegram_connections, telegram_message_links, telegram_threads,
        threads,
    },
    runner::{
        AgentEvent, AgentEventKind, AgentRequest, RUNNER_LIFECYCLE_TOPIC, RequestSource,
        RunnerLifecycle, thread_events_topic,
    },
    tools::telegram::{TELEGRAM_MESSAGE_LIMIT, TELEGRAM_RICH_MESSAGE_LIMIT, split_message},
    vfs::{WORKSPACE_MOUNT, WritableArea},
};

/// How long a streamed Telegram draft waits before the next update is pushed.
const DRAFT_INTERVAL: Duration = Duration::from_millis(700);

/// Telegram login payloads older than this are rejected to prevent replay of a
/// captured `hash`. Matches Telegram's own recommendation in the Login Widget docs.
const LOGIN_MAX_AGE_SECONDS: i64 = 86_400;
const TELEGRAM_SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";

#[derive(Serialize)]
pub struct TelegramSettingsResponse {
    bot_configured: bool,
    bot_username: Option<String>,
    connected: bool,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

pub struct TelegramSentMessage {
    pub chat_id: i64,
    pub message_id: i64,
}

#[derive(Debug)]
pub enum TelegramApiError {
    Auth(AuthError),
    Unauthorized,
    NotFound,
    Internal,
}

impl IntoResponse for TelegramApiError {
    fn into_response(self) -> Response {
        match self {
            TelegramApiError::Auth(error) => error.into_response(),
            TelegramApiError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            TelegramApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            TelegramApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for TelegramApiError {
    fn from(error: AuthError) -> Self {
        TelegramApiError::Auth(error)
    }
}

pub async fn settings(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<TelegramSettingsResponse>, TelegramApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    let connection = connection_for_user(&state, user_id).await?;
    let bot_username = telegram_bot_username(&state).await;

    Ok(Json(TelegramSettingsResponse {
        bot_configured: bot_token(&state).is_some(),
        bot_username,
        connected: connection.is_some(),
        username: connection.as_ref().and_then(|c| c.username.clone()),
        first_name: connection.as_ref().and_then(|c| c.first_name.clone()),
        last_name: connection.and_then(|c| c.last_name),
    }))
}

/// Completes the Telegram Login Widget flow. The browser receives a signed user
/// object from `oauth.telegram.org`, posts it here, and we link it to the
/// authenticated Stride account only after verifying the signature with the bot
/// token. See https://core.telegram.org/bots/telegram-login.
pub async fn login(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<StatusCode, TelegramApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    let Some(token) = bot_token(&state) else {
        return Err(TelegramApiError::NotFound);
    };

    let Some(fields) = payload.as_object() else {
        return Err(TelegramApiError::Unauthorized);
    };

    if !verify_login(&token, fields) {
        return Err(TelegramApiError::Unauthorized);
    }

    let auth_date = fields
        .get("auth_date")
        .and_then(Value::as_i64)
        .ok_or(TelegramApiError::Unauthorized)?;
    if (now() - auth_date).abs() > LOGIN_MAX_AGE_SECONDS {
        return Err(TelegramApiError::Unauthorized);
    }

    let telegram_user_id = fields
        .get("id")
        .and_then(Value::as_i64)
        .ok_or(TelegramApiError::Unauthorized)?;
    let username = fields.get("username").and_then(Value::as_str);
    let first_name = fields.get("first_name").and_then(Value::as_str);
    let last_name = fields.get("last_name").and_then(Value::as_str);

    telegram_connections::delete()
        .where_(
            telegram_connections::user_id
                .eq(user_id)
                .or(telegram_connections::telegram_user_id.eq(telegram_user_id)),
        )
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    telegram_connections::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .telegram_user_id(telegram_user_id)
        .chat_id(telegram_user_id)
        .username(username)
        .first_name(first_name)
        .last_name(last_name)
        .connected_at(now())
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn disconnect(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<StatusCode, TelegramApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;

    telegram_connections::delete()
        .where_(telegram_connections::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn webhook(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, TelegramApiError> {
    validate_secret(&state, &headers)?;

    let mut update: TelegramUpdate = match serde_json::from_slice(&body) {
        Ok(update) => update,
        Err(error) => {
            // Returning OK (not 422) keeps Telegram from retrying an update we will never accept.
            tracing::warn!(%error, "ignoring unparseable Telegram update");
            return Ok(StatusCode::OK);
        }
    };

    if let Some(callback) = update.callback_query.take() {
        handle_callback(&state, callback).await;
        return Ok(StatusCode::OK);
    }

    let Some(message) = update.message() else {
        return Ok(StatusCode::OK);
    };

    handle_topic_message(state, message).await?;
    Ok(StatusCode::OK)
}

async fn handle_topic_message(
    state: Arc<ServerState>,
    message: TelegramMessage,
) -> Result<(), TelegramApiError> {
    if message.forum_topic_created.is_some() {
        return Ok(());
    }

    let Some(from) = message.from.as_ref() else {
        return Ok(());
    };

    let text = message.message_text();
    let attachments = message.attachments();
    let voice = message.voice_note();
    if text.is_none() && attachments.is_empty() && voice.is_none() {
        return Ok(());
    }

    let Some(user_id) = user_for_telegram_id(&state, from.id).await? else {
        if message.chat.kind == "private" && text.is_some_and(is_start_command) {
            send_telegram_message(
                &state,
                message.chat.id,
                message.send_topic_id(),
                "Open Stride Settings and connect your Telegram account with the login button.",
            )
            .await;
        }
        return Ok(());
    };

    // Interactive flows (button replies, slash commands, free-form quiz answers) only apply to
    // plain text messages; a message carrying files always starts or continues an agent run.
    if attachments.is_empty()
        && let Some(text) = text
    {
        if resolve_text_action(&state, &message, text).await? {
            return Ok(());
        }

        if text.starts_with('/') {
            return Ok(());
        }

        if let Some((quiz_id, question_index)) =
            pending_free_form_quiz(&state, message.chat.id, message.send_topic_id())
        {
            answer_quiz_question(&state, quiz_id, question_index, text.to_string()).await;
            return Ok(());
        }
    }

    // A voice note has no typed text: the recorded audio *is* the message. Transcribe it up front
    // and treat the transcript as the user's words; the agent never sees the audio file.
    let transcript = if let Some(voice) = &voice {
        match transcribe_voice_note(&state, voice).await {
            Some(transcript) => Some(transcript),
            None => {
                send_telegram_message(
                    &state,
                    message.chat.id,
                    message.send_topic_id(),
                    voice_transcription_error(&state),
                )
                .await;
                return Ok(());
            }
        }
    } else {
        None
    };
    let body = transcript.as_deref().or(text);

    let (thread_id, is_new) =
        if let Some(thread_id) = reply_thread(&state, user_id, &message).await? {
            (thread_id, false)
        } else {
            ensure_telegram_thread(&state, user_id, &message).await?
        };

    if is_new {
        let title_seed = body
            .map(str::to_string)
            .unwrap_or_else(|| attachment_title_seed(&attachments));
        crate::api::threads::spawn_title_generation(
            state.clone(),
            thread_id,
            title_seed,
            message
                .message_thread_id
                .map(|topic| (message.chat.id, topic)),
        );
    }
    link_telegram_message(
        &state,
        user_id,
        message.chat.id,
        message.message_id,
        thread_id,
    )
    .await?;
    ensure_telegram_mapping_for_message(&state, user_id, &message, thread_id).await;

    let content = build_agent_content(&state, user_id, thread_id, body, &attachments).await;

    // The pool serializes concurrent messages per thread, so a plain send never collides.
    if let Err(error) = state
        .runner
        .send(
            thread_id,
            AgentRequest {
                content,
                images: Vec::new(),
                model: None,
                source: RequestSource::Human,
            },
        )
        .await
    {
        tracing::warn!(%thread_id, %error, "failed to start Telegram agent run");
        send_telegram_message(
            &state,
            message.chat.id,
            message.send_topic_id(),
            "Stride could not start: please try again.",
        )
        .await;
    }

    Ok(())
}

/// Hands the agent the user's text plus, for any attached files, a note of where each landed in
/// the workspace after being downloaded (or that the download failed).
async fn build_agent_content(
    state: &ServerState,
    user_id: Uuid,
    thread_id: Uuid,
    text: Option<&str>,
    attachments: &[IncomingAttachment],
) -> String {
    let mut content = text.unwrap_or_default().to_string();
    if attachments.is_empty() {
        return content;
    }

    let saved = download_attachments_to_workspace(state, user_id, thread_id, attachments).await;
    let note = if saved.is_empty() {
        format!(
            "[The user attached {} file(s), but Stride could not download them.]",
            attachments.len()
        )
    } else {
        let list = saved.join(", ");
        format!(
            "[The user attached {} file(s), saved to the workspace: {list}]",
            saved.len()
        )
    };
    if !content.is_empty() {
        content.push_str("\n\n");
    }
    content.push_str(&note);
    content
}

/// Transcribes audio bytes using the registered transcription model, returning
/// the spoken text or `None` when no model is configured or the request fails.
async fn transcribe_audio(
    state: &ServerState,
    bytes: &[u8],
    file_name: &str,
    mime_type: &str,
) -> Option<String> {
    let model = state.model_config.model_registry.transcription()?;
    match model
        .api
        .transcribe(&model.token, bytes, file_name, mime_type, &model.model_name)
        .await
    {
        Ok(transcription) => {
            let text = transcription.text.trim().to_string();
            (!text.is_empty()).then_some(text)
        }
        Err(error) => {
            tracing::warn!(%error, "failed to transcribe Telegram voice message");
            None
        }
    }
}

/// Downloads a Telegram voice note and returns its transcription. The recorded
/// audio is the message itself, so it is transcribed in memory and never stored.
async fn transcribe_voice_note(state: &ServerState, voice: &VoiceNote) -> Option<String> {
    let token = bot_token(state)?;
    let bytes = download_telegram_file(state, &token, &voice.file_id).await?;
    transcribe_audio(state, &bytes, "voice.ogg", &voice.mime_type).await
}

/// The reply shown when a voice note cannot be turned into text, distinguishing a
/// missing transcription model from a transcription that failed.
fn voice_transcription_error(state: &ServerState) -> &'static str {
    if state.model_config.model_registry.transcription().is_some() {
        "Sorry, I couldn't transcribe that voice message. Please try again."
    } else {
        "Voice messages aren't supported yet: no transcription model is configured."
    }
}

fn attachment_title_seed(attachments: &[IncomingAttachment]) -> String {
    match attachments.first() {
        Some(first) => format!("Shared file {}", first.file_name),
        None => "Shared a file".to_string(),
    }
}

/// Downloads each attachment from Telegram and writes it into the thread's
/// writable directory under `uploads/`, returning the agent-facing absolute
/// paths (e.g. `/Projects/Acme/uploads/photo.jpg`) that were stored.
async fn download_attachments_to_workspace(
    state: &ServerState,
    user_id: Uuid,
    thread_id: Uuid,
    attachments: &[IncomingAttachment],
) -> Vec<String> {
    let Some(vfs) = state.vfs.as_ref() else {
        tracing::warn!(%thread_id, "no VFS configured; cannot save Telegram attachments");
        return Vec::new();
    };
    let Some(token) = bot_token(state) else {
        return Vec::new();
    };
    let Some((area, root)) = thread_writable_area(state, vfs, user_id, thread_id).await else {
        tracing::warn!(%thread_id, "failed to open writable area for Telegram attachments");
        return Vec::new();
    };

    let mut saved = Vec::new();
    for attachment in attachments {
        let Some(bytes) = download_telegram_file(state, &token, &attachment.file_id).await else {
            continue;
        };

        let rel = format!("uploads/{}", attachment.file_name);
        match vfs
            .area_write_bytes(
                &area,
                user_id,
                &rel,
                &bytes,
                attachment.mime_type.as_deref(),
            )
            .await
        {
            Ok(()) => saved.push(format!("{root}/{rel}")),
            Err(error) => {
                tracing::warn!(%thread_id, rel, %error, "failed to write Telegram attachment");
            }
        }
    }
    saved
}

/// Resolves a Telegram thread's writable area and the absolute path the agent
/// uses to reach it. Project threads write into the project's folder in the
/// user's global files; others keep a standalone workspace.
async fn thread_writable_area(
    state: &ServerState,
    vfs: &crate::vfs::Vfs,
    user_id: Uuid,
    thread_id: Uuid,
) -> Option<(WritableArea, String)> {
    if let Some(pid) = thread_project_id(&state.db, thread_id).await
        && let Some(title) = project_title(&state.db, pid).await
        && let Ok(prefix) = vfs.ensure_project_dir(user_id, &title).await
    {
        let root = format!("/{prefix}");
        return Some((WritableArea::ProjectDir(prefix), root));
    }
    let workspace_id = vfs
        .get_or_create_workspace(thread_id, None, user_id)
        .await
        .ok()?;
    Some((
        WritableArea::Workspace(workspace_id),
        format!("/{WORKSPACE_MOUNT}"),
    ))
}

async fn project_title(db: &ConnectionPool, project_id: Uuid) -> Option<String> {
    projects::select_cols((projects::title,))
        .where_(projects::id.eq(project_id))
        .all(db)
        .await
        .ok()?
        .into_iter()
        .next()
        .map(|(title,)| title)
        .filter(|title| !title.is_empty())
}

async fn thread_project_id(db: &ConnectionPool, thread_id: Uuid) -> Option<Uuid> {
    threads::select_cols((threads::project_id,))
        .where_(threads::id.eq(thread_id))
        .all(db)
        .await
        .ok()?
        .into_iter()
        .next()
        .and_then(|(project_id,)| project_id)
}

/// Resolves a Telegram `file_id` to its bytes via `getFile` + the file download endpoint. Telegram
/// caps bot downloads at 20 MB; larger files fail `getFile` and yield `None`.
async fn download_telegram_file(
    state: &ServerState,
    token: &str,
    file_id: &str,
) -> Option<Vec<u8>> {
    let body = serde_json::to_vec(&json!({ "file_id": file_id })).ok()?;
    let response = telegram_post(state, "getFile", body).await?;
    let file_path = serde_json::from_slice::<TelegramApiResponse<TelegramFile>>(&response)
        .ok()?
        .result?
        .file_path?;

    let uri = format!("https://api.telegram.org/file/bot{token}/{file_path}");
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .ok()?;
    let (status, body) = match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to download Telegram file");
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, "timed out downloading Telegram file");
            return None;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(status, "Telegram file download returned error");
        return None;
    }
    Some(body.to_vec())
}

/// Pending interactive prompts (approvals and quizzes) shown in Telegram as buttons or, for
/// free-form quiz questions, captured from the user's next typed reply.
#[derive(Default)]
pub(crate) struct Interactions {
    /// Button `callback_data` token → the action that tap performs.
    callbacks: HashMap<String, CallbackAction>,
    /// Reply keyboard text in a chat/topic → the action that text performs.
    text_actions: HashMap<(i64, Option<i64>, String), CallbackAction>,
    /// In-flight quizzes, keyed by quiz id, collecting one answer per question.
    quizzes: HashMap<Uuid, QuizState>,
    /// (chat_id, topic_id) currently awaiting a typed free-form answer → quiz id.
    awaiting_text: HashMap<(i64, Option<i64>), Uuid>,
}

#[derive(Clone)]
enum CallbackAction {
    Approval {
        thread_id: Uuid,
        approval_id: Uuid,
        approved: bool,
        sibling: String,
    },
    QuizOption {
        thread_id: Uuid,
        quiz_id: Uuid,
        question_index: usize,
        answer: String,
    },
}

impl CallbackAction {
    fn thread_id(&self) -> Uuid {
        match self {
            CallbackAction::Approval { thread_id, .. } => *thread_id,
            CallbackAction::QuizOption { thread_id, .. } => *thread_id,
        }
    }
}

struct QuizState {
    thread_id: Uuid,
    chat_id: i64,
    topic_id: Option<i64>,
    questions: Vec<QuizQuestion>,
    answers: Vec<Option<String>>,
    current: usize,
    /// Button tokens registered for the question currently awaiting an answer.
    tokens: Vec<String>,
}

/// Resolves the Telegram chat a thread replies to, plus the thread owner. Only threads with an
/// explicit Telegram topic/private-chat mapping are forwarded to Telegram.
async fn telegram_destination(
    db: &ConnectionPool,
    thread_id: Uuid,
) -> Option<(i64, Option<i64>, Uuid)> {
    let (user_id,) = threads::select_cols((threads::owner,))
        .where_(threads::id.eq(thread_id))
        .all(db)
        .await
        .ok()?
        .into_iter()
        .next()?;

    let (chat_id, topic_id) =
        telegram_threads::select_cols((telegram_threads::chat_id, telegram_threads::topic_id))
            .where_(telegram_threads::thread_id.eq(thread_id))
            .all(db)
            .await
            .ok()?
            .into_iter()
            .next()?;
    Some((chat_id, (topic_id != 0).then_some(topic_id), user_id))
}

async fn thread_has_telegram_mapping(db: &ConnectionPool, thread_id: Uuid) -> bool {
    telegram_threads::select_cols((telegram_threads::thread_id,))
        .where_(telegram_threads::thread_id.eq(thread_id))
        .all(db)
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .is_some()
}

async fn ensure_telegram_mapping_for_message(
    state: &ServerState,
    user_id: Uuid,
    message: &TelegramMessage,
    thread_id: Uuid,
) {
    if thread_has_telegram_mapping(&state.db, thread_id).await {
        return;
    }

    let topic_id = message.storage_topic_id();
    match telegram_threads::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .chat_id(message.chat.id)
        .topic_id(topic_id)
        .thread_id(thread_id)
        .execute(&state.db)
        .await
    {
        Ok(_) => tracing::info!(
            %thread_id,
            chat_id = message.chat.id,
            topic_id,
            "created Telegram thread mapping"
        ),
        Err(error) => tracing::warn!(
            %thread_id,
            chat_id = message.chat.id,
            topic_id,
            %error,
            "failed to create Telegram thread mapping"
        ),
    }
}

/// Forwards a Telegram thread's agent events straight to Telegram. Created at runner start (one per
/// thread), so it reliably sees every event pushed to it — there is no on-demand subscription to
/// race or miss.
/// Forwards one Telegram-originated thread's agent events to Telegram. A single task owns it, so the
/// per-run state is a plain field rather than a shared lock.
struct TelegramSubscriber {
    state: Arc<ServerState>,
    thread_id: Uuid,
    active: Option<ActiveRun>,
}

struct ActiveRun {
    run_id: Uuid,
    user_id: Uuid,
    chat_id: i64,
    topic_id: Option<i64>,
    draft_id: i64,
    content: String,
    thinking: String,
    last_draft_text: String,
    last_draft: Instant,
    finalized: bool,
}

impl TelegramSubscriber {
    async fn handle_event(&mut self, event: &AgentEvent) {
        let Some(run_id) = event.run_id.map(|id| id.0) else {
            return;
        };

        if matches!(event.kind, AgentEventKind::RunStarted) {
            let Some((chat_id, topic_id, user_id)) =
                telegram_destination(&self.state.db, self.thread_id).await
            else {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    %run_id,
                    "Telegram subscriber has no destination"
                );
                return;
            };
            self.active = Some(ActiveRun {
                run_id,
                user_id,
                chat_id,
                topic_id,
                draft_id: telegram_draft_id(run_id),
                content: String::new(),
                thinking: String::new(),
                last_draft_text: String::new(),
                last_draft: Instant::now(),
                finalized: false,
            });
            if let Some(active) = self.active.as_mut() {
                let draft = telegram_draft_markdown("", &active.thinking);
                active.last_draft_text = draft.clone();
                let (chat_id, topic_id, draft_id) =
                    (active.chat_id, active.topic_id, active.draft_id);
                let state = self.state.clone();
                tokio::spawn(async move {
                    send_telegram_rich_message_draft(&state, chat_id, topic_id, draft_id, &draft)
                        .await;
                });
            }
            return;
        }

        match &event.kind {
            AgentEventKind::AgentDelta { content, .. } => {
                let draft = {
                    let Some(active) = self.active.as_mut().filter(|a| a.run_id == run_id) else {
                        return;
                    };
                    active.content = content.clone();
                    let draft = telegram_draft_markdown(&active.content, &active.thinking);
                    if draft != active.last_draft_text
                        && active.last_draft.elapsed() >= DRAFT_INTERVAL
                    {
                        active.last_draft = Instant::now();
                        active.last_draft_text = draft.clone();
                        Some((active.chat_id, active.topic_id, active.draft_id, draft))
                    } else {
                        None
                    }
                };
                if let Some((chat_id, topic_id, draft_id, draft)) = draft {
                    let state = self.state.clone();
                    tokio::spawn(async move {
                        send_telegram_rich_message_draft(
                            &state, chat_id, topic_id, draft_id, &draft,
                        )
                        .await;
                    });
                }
            }
            AgentEventKind::ThinkingDelta { thinking } => {
                let draft = {
                    let Some(active) = self.active.as_mut().filter(|a| a.run_id == run_id) else {
                        return;
                    };
                    let first_real_thinking = active.thinking.trim().is_empty();
                    active.thinking.push_str(thinking);
                    let draft = telegram_draft_markdown(&active.content, &active.thinking);
                    if draft != active.last_draft_text
                        && (first_real_thinking || active.last_draft.elapsed() >= DRAFT_INTERVAL)
                    {
                        active.last_draft = Instant::now();
                        active.last_draft_text = draft.clone();
                        Some((active.chat_id, active.topic_id, active.draft_id, draft))
                    } else {
                        None
                    }
                };
                if let Some((chat_id, topic_id, draft_id, draft)) = draft {
                    let state = self.state.clone();
                    tokio::spawn(async move {
                        send_telegram_rich_message_draft(
                            &state, chat_id, topic_id, draft_id, &draft,
                        )
                        .await;
                    });
                }
            }
            AgentEventKind::AgentMessageCommitted { message_id, .. } => {
                let Some((user_id, chat_id, topic_id)) = self.active_run(run_id) else {
                    return;
                };
                if let Some(content) = agent_message_content(&self.state, *message_id).await {
                    finalize_telegram_stream(
                        &self.state,
                        user_id,
                        self.thread_id,
                        chat_id,
                        topic_id,
                        &content,
                    )
                    .await;
                    if let Some(active) = self.active.as_mut() {
                        active.content = content;
                        active.thinking.clear();
                        active.finalized = true;
                    }
                }
            }
            AgentEventKind::RunFinished => {
                let info = self
                    .active
                    .as_ref()
                    .filter(|a| a.run_id == run_id)
                    .map(|a| {
                        (
                            a.user_id,
                            a.chat_id,
                            a.topic_id,
                            a.content.clone(),
                            a.finalized,
                        )
                    });
                if let Some((user_id, chat_id, topic_id, mut content, finalized)) = info {
                    if !finalized {
                        finalize_latest_telegram_response(
                            &self.state,
                            user_id,
                            self.thread_id,
                            chat_id,
                            topic_id,
                            &mut content,
                        )
                        .await;
                    }
                    self.end_run(chat_id, topic_id);
                }
            }
            AgentEventKind::RunFailed { error } => {
                if let Some((_, chat_id, topic_id)) = self.active_run(run_id) {
                    send_telegram_message(
                        &self.state,
                        chat_id,
                        topic_id,
                        &format!("Stride failed: {error}"),
                    )
                    .await;
                    self.end_run(chat_id, topic_id);
                }
            }
            AgentEventKind::RunCancelled => {
                if let Some((_, chat_id, topic_id)) = self.active_run(run_id) {
                    self.end_run(chat_id, topic_id);
                }
            }
            AgentEventKind::WaitingForApproval {
                approval_id,
                message,
            } => {
                if let Some((_, chat_id, topic_id)) = self.active_run(run_id) {
                    tracing::info!(
                        thread_id = %self.thread_id,
                        %approval_id,
                        chat_id,
                        ?topic_id,
                        "presenting Telegram approval"
                    );
                    present_approval(
                        &self.state,
                        self.thread_id,
                        chat_id,
                        topic_id,
                        *approval_id,
                        message,
                    )
                    .await;
                }
            }
            AgentEventKind::WaitingForQuiz { quiz_id, questions } => {
                if let Some((_, chat_id, topic_id)) = self.active_run(run_id) {
                    tracing::info!(
                        thread_id = %self.thread_id,
                        %quiz_id,
                        chat_id,
                        ?topic_id,
                        question_count = questions.len(),
                        "presenting Telegram quiz"
                    );
                    present_quiz(
                        &self.state,
                        self.thread_id,
                        chat_id,
                        topic_id,
                        *quiz_id,
                        questions.clone(),
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    /// Returns (user_id, chat_id, topic_id) when `run_id` is the run currently being forwarded.
    fn active_run(&self, run_id: Uuid) -> Option<(Uuid, i64, Option<i64>)> {
        self.active
            .as_ref()
            .filter(|a| a.run_id == run_id)
            .map(|a| (a.user_id, a.chat_id, a.topic_id))
    }

    fn end_run(&mut self, chat_id: i64, topic_id: Option<i64>) {
        clear_interactions(&self.state, self.thread_id, chat_id, topic_id);
        self.active = None;
    }
}

/// Forwards a Telegram thread's events until its runner is evicted (the topic closes). Its lifetime
/// is owned by [`supervise`], which binds it to the agent runner's lifecycle.
async fn run_telegram_subscriber(state: Arc<ServerState>, thread_id: Uuid) {
    let mut subscriber = TelegramSubscriber {
        state,
        thread_id,
        active: None,
    };
    let mut events = pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).subscribe();
    loop {
        match events.recv().await {
            Ok(event) => subscriber.handle_event(&event).await,
            Err(pubsub::RecvError::Lagged(_)) => {}
            Err(pubsub::RecvError::Closed) => break,
        }
    }
}

/// Binds Telegram subscriber tasks to agent runner lifetimes. Listens for [`RunnerLifecycle`] and,
/// for Telegram-mapped threads, keeps exactly one subscriber task alive while the runner exists —
/// aborting it when the runner is evicted, so subscribers do not accumulate.
pub(crate) async fn supervise(state: Arc<ServerState>) {
    let mut tasks: HashMap<Uuid, tokio::task::AbortHandle> = HashMap::new();
    let mut lifecycle = pubsub::topic::<RunnerLifecycle>(RUNNER_LIFECYCLE_TOPIC).subscribe();

    loop {
        let event = match lifecycle.recv().await {
            Ok(event) => event,
            Err(pubsub::RecvError::Lagged(_)) => continue,
            Err(pubsub::RecvError::Closed) => break,
        };

        match event {
            RunnerLifecycle::Activated { thread_id } => {
                if !thread_has_telegram_mapping(&state.db, thread_id).await {
                    continue;
                }
                if let Some(stale) = tasks.remove(&thread_id) {
                    stale.abort();
                    tracing::info!(%thread_id, "replacing stale Telegram subscriber on re-activation");
                }
                let handle = tokio::spawn(run_telegram_subscriber(state.clone(), thread_id));
                tasks.insert(thread_id, handle.abort_handle());
                tracing::info!(%thread_id, "spawned Telegram subscriber");
            }
            RunnerLifecycle::Deactivated { thread_id } => {
                if let Some(handle) = tasks.remove(&thread_id) {
                    handle.abort();
                    tracing::info!(%thread_id, "aborted Telegram subscriber on runner eviction");
                }
            }
        }
    }
}

async fn present_approval(
    state: &ServerState,
    thread_id: Uuid,
    chat_id: i64,
    topic_id: Option<i64>,
    approval_id: Uuid,
    message: &str,
) {
    let approve = interaction_token();
    let deny = interaction_token();
    {
        let mut ix = state.telegram_interactions.lock().unwrap();
        let approve_action = CallbackAction::Approval {
            thread_id,
            approval_id,
            approved: true,
            sibling: deny.clone(),
        };
        let deny_action = CallbackAction::Approval {
            thread_id,
            approval_id,
            approved: false,
            sibling: approve.clone(),
        };
        ix.callbacks.insert(approve.clone(), approve_action.clone());
        ix.callbacks.insert(deny.clone(), deny_action.clone());
        ix.text_actions
            .insert((chat_id, topic_id, "Approve".to_string()), approve_action);
        ix.text_actions
            .insert((chat_id, topic_id, "Deny".to_string()), deny_action);
    }

    let keyboard = vec![vec![
        InlineButton {
            text: "Approve".to_string(),
            callback_data: approve,
        },
        InlineButton {
            text: "Deny".to_string(),
            callback_data: deny,
        },
    ]];
    send_telegram_buttons(state, chat_id, topic_id, &format!("⚠️ {message}"), keyboard).await;
}

async fn resolve_text_action(
    state: &ServerState,
    message: &TelegramMessage,
    text: &str,
) -> Result<bool, TelegramApiError> {
    let topic_id = message.send_topic_id();
    let action = {
        let ix = state.telegram_interactions.lock().unwrap();
        ix.text_actions
            .get(&(message.chat.id, topic_id, text.to_string()))
            .cloned()
    };
    let Some(action) = action else {
        return Ok(false);
    };

    let thread_id = action.thread_id();
    let owner = thread_owner(state, thread_id).await;
    let caller = match message.from.as_ref() {
        Some(from) => user_for_telegram_id(state, from.id).await?,
        None => None,
    };
    if owner.is_none() || owner != caller {
        tracing::warn!(
            %thread_id,
            ?owner,
            ?caller,
            "Telegram text button rejected: caller is not the thread owner"
        );
        send_telegram_message(state, message.chat.id, topic_id, "Not allowed.").await;
        return Ok(true);
    }

    match action {
        CallbackAction::Approval {
            thread_id,
            approval_id,
            approved,
            sibling,
        } => {
            {
                let mut ix = state.telegram_interactions.lock().unwrap();
                ix.callbacks.remove(&sibling);
                ix.callbacks
                    .retain(|_, action| action.thread_id() != thread_id);
                ix.text_actions
                    .retain(|_, action| action.thread_id() != thread_id);
            }

            if let Err(error) = state
                .runner
                .resolve_approval(thread_id, approval_id, approved)
                .await
            {
                tracing::warn!(%thread_id, %approval_id, %error, "failed to resolve Telegram approval");
            }
            send_telegram_message(
                state,
                message.chat.id,
                topic_id,
                if approved { "Approved" } else { "Denied" },
            )
            .await;
        }
        CallbackAction::QuizOption {
            quiz_id,
            question_index,
            answer,
            ..
        } => {
            answer_quiz_question(state, quiz_id, question_index, answer).await;
        }
    }

    Ok(true)
}

async fn present_quiz(
    state: &ServerState,
    thread_id: Uuid,
    chat_id: i64,
    topic_id: Option<i64>,
    quiz_id: Uuid,
    questions: Vec<QuizQuestion>,
) {
    tracing::info!(
        %thread_id,
        %quiz_id,
        chat_id,
        ?topic_id,
        question_count = questions.len(),
        "registered Telegram quiz"
    );
    // Empty quizzes are resolved by the agent itself, so questions is always non-empty here.
    {
        let mut ix = state.telegram_interactions.lock().unwrap();
        ix.quizzes.insert(
            quiz_id,
            QuizState {
                thread_id,
                chat_id,
                topic_id,
                answers: vec![None; questions.len()],
                questions,
                current: 0,
                tokens: Vec::new(),
            },
        );
    }
    send_quiz_question(state, quiz_id).await;
}

/// Sends the quiz's current question: inline buttons for option questions, or a plain prompt that
/// captures the user's next typed reply for free-form questions.
async fn send_quiz_question(state: &ServerState, quiz_id: Uuid) {
    let Some(prompt) = ({
        let mut ix = state.telegram_interactions.lock().unwrap();
        let Some((index, question, chat_id, topic_id, count, thread_id)) =
            ix.quizzes.get(&quiz_id).map(|quiz| {
                (
                    quiz.current,
                    quiz.questions[quiz.current].clone(),
                    quiz.chat_id,
                    quiz.topic_id,
                    quiz.questions.len(),
                    quiz.thread_id,
                )
            })
        else {
            return;
        };
        let header = format!("❓ ({}/{count}) {}", index + 1, question.question);
        tracing::info!(
            %quiz_id,
            chat_id,
            ?topic_id,
            question_index = index,
            option_count = question.options.len(),
            "sending Telegram quiz question"
        );

        if question.options.is_empty() {
            ix.awaiting_text.insert((chat_id, topic_id), quiz_id);
            if let Some(quiz) = ix.quizzes.get_mut(&quiz_id) {
                quiz.tokens.clear();
            }
            Some(QuizPrompt {
                chat_id,
                topic_id,
                text: format!("{header}\n\nReply to this chat with your answer."),
                keyboard: None,
            })
        } else {
            ix.awaiting_text.remove(&(chat_id, topic_id));
            let mut tokens = Vec::new();
            let mut keyboard = Vec::new();
            for option in &question.options {
                let token = interaction_token();
                ix.callbacks.insert(
                    token.clone(),
                    CallbackAction::QuizOption {
                        thread_id,
                        quiz_id,
                        question_index: index,
                        answer: option.clone(),
                    },
                );
                ix.text_actions.insert(
                    (chat_id, topic_id, option.clone()),
                    CallbackAction::QuizOption {
                        thread_id,
                        quiz_id,
                        question_index: index,
                        answer: option.clone(),
                    },
                );
                tokens.push(token.clone());
                keyboard.push(vec![InlineButton {
                    text: option.clone(),
                    callback_data: token,
                }]);
            }
            if let Some(quiz) = ix.quizzes.get_mut(&quiz_id) {
                quiz.tokens = tokens;
            }
            Some(QuizPrompt {
                chat_id,
                topic_id,
                text: header,
                keyboard: Some(keyboard),
            })
        }
    }) else {
        return;
    };

    match prompt.keyboard {
        Some(keyboard) => {
            send_telegram_buttons(
                state,
                prompt.chat_id,
                prompt.topic_id,
                &prompt.text,
                keyboard,
            )
            .await;
        }
        None => {
            send_telegram_message(state, prompt.chat_id, prompt.topic_id, &prompt.text).await;
        }
    }
}

struct QuizPrompt {
    chat_id: i64,
    topic_id: Option<i64>,
    text: String,
    keyboard: Option<Vec<Vec<InlineButton>>>,
}

struct AnswerQuizResult {
    chat_id: i64,
    topic_id: Option<i64>,
    submit: Option<(Uuid, Vec<String>)>,
}

/// Records an answer for `question_index`, then either advances to the next question or submits the
/// completed quiz to the agent.
async fn answer_quiz_question(
    state: &ServerState,
    quiz_id: Uuid,
    question_index: usize,
    answer: String,
) {
    let result = {
        let mut ix = state.telegram_interactions.lock().unwrap();
        let Some(quiz) = ix.quizzes.get_mut(&quiz_id) else {
            return;
        };
        if question_index != quiz.current {
            return;
        }
        let stale_tokens = std::mem::take(&mut quiz.tokens);
        quiz.answers[quiz.current] = Some(answer);
        quiz.current += 1;
        let done = quiz.current >= quiz.questions.len();
        let thread_id = quiz.thread_id;
        let chat_id = quiz.chat_id;
        let topic_id = quiz.topic_id;
        let answers = done.then(|| {
            quiz.answers
                .iter()
                .map(|a| a.clone().unwrap_or_default())
                .collect::<Vec<_>>()
        });

        for token in stale_tokens {
            ix.callbacks.remove(&token);
        }
        ix.text_actions.retain(|_, action| {
            !matches!(
                action,
                CallbackAction::QuizOption {
                    quiz_id: id,
                    question_index: index,
                    ..
                } if *id == quiz_id && *index == question_index
            )
        });
        if done {
            ix.quizzes.remove(&quiz_id);
            ix.awaiting_text.remove(&(chat_id, topic_id));
            AnswerQuizResult {
                chat_id,
                topic_id,
                submit: answers.map(|answers| (thread_id, answers)),
            }
        } else {
            AnswerQuizResult {
                chat_id,
                topic_id,
                submit: None,
            }
        }
    };

    remove_telegram_keyboard(state, result.chat_id, result.topic_id).await;

    match result.submit {
        Some((thread_id, answers)) => {
            tracing::info!(
                %thread_id,
                %quiz_id,
                answer_count = answers.len(),
                "answering Telegram quiz"
            );
            if let Err(error) = state.runner.answer_quiz(thread_id, quiz_id, answers).await {
                tracing::warn!(%thread_id, %quiz_id, %error, "failed to answer Telegram quiz");
            }
        }
        None => send_quiz_question(state, quiz_id).await,
    }
}

fn pending_free_form_quiz(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
) -> Option<(Uuid, usize)> {
    let ix = state.telegram_interactions.lock().unwrap();
    let quiz_id = *ix.awaiting_text.get(&(chat_id, topic_id))?;
    let quiz = ix.quizzes.get(&quiz_id)?;
    Some((quiz_id, quiz.current))
}

async fn handle_callback(state: &ServerState, callback: CallbackQuery) {
    let Some(token) = callback.data.clone() else {
        tracing::warn!("Telegram callback has no data token");
        return;
    };
    let (action, registered) = {
        let ix = state.telegram_interactions.lock().unwrap();
        (ix.callbacks.get(&token).cloned(), ix.callbacks.len())
    };
    let Some(action) = action else {
        tracing::warn!(
            %token,
            from_id = callback.from.id,
            registered,
            "Telegram callback token not found (cleared, or never registered in this map)"
        );
        answer_callback_query(state, &callback.id, "This action is no longer available.").await;
        return;
    };

    // Only the thread owner may resolve a prompt — buttons can be visible to a whole group.
    let thread_id = action.thread_id();
    let owner = thread_owner(state, thread_id).await;
    let caller = user_for_telegram_id(state, callback.from.id)
        .await
        .ok()
        .flatten();
    if owner.is_none() || owner != caller {
        tracing::warn!(
            %thread_id,
            from_id = callback.from.id,
            ?owner,
            ?caller,
            "Telegram callback rejected: caller is not the thread owner"
        );
        answer_callback_query(state, &callback.id, "Not allowed.").await;
        return;
    }
    tracing::info!(%thread_id, %token, "handling Telegram callback");

    match action {
        CallbackAction::Approval {
            thread_id,
            approval_id,
            approved,
            sibling,
        } => {
            {
                let mut ix = state.telegram_interactions.lock().unwrap();
                ix.callbacks.remove(&token);
                ix.callbacks.remove(&sibling);
            }
            if let Err(error) = state
                .runner
                .resolve_approval(thread_id, approval_id, approved)
                .await
            {
                tracing::warn!(%thread_id, %approval_id, %error, "failed to resolve Telegram approval");
            }
            answer_callback_query(
                state,
                &callback.id,
                if approved { "Approved" } else { "Denied" },
            )
            .await;
            if let Some(message) = &callback.message {
                let label = if approved {
                    "✅ Approved"
                } else {
                    "❌ Denied"
                };
                edit_telegram_message(state, message.chat.id, message.message_id, label).await;
            }
        }
        CallbackAction::QuizOption {
            quiz_id,
            question_index,
            answer,
            ..
        } => {
            answer_quiz_question(state, quiz_id, question_index, answer.clone()).await;
            answer_callback_query(state, &callback.id, &answer).await;
            if let Some(message) = &callback.message {
                let question = message.text.clone().unwrap_or_default();
                edit_telegram_message(
                    state,
                    message.chat.id,
                    message.message_id,
                    &format!("{question}\n➡️ {answer}"),
                )
                .await;
            }
        }
    }
}

fn clear_interactions(state: &ServerState, thread_id: Uuid, chat_id: i64, topic_id: Option<i64>) {
    let mut ix = state.telegram_interactions.lock().unwrap();
    let before = ix.callbacks.len();
    ix.callbacks
        .retain(|_, action| action.thread_id() != thread_id);
    ix.text_actions
        .retain(|_, action| action.thread_id() != thread_id);
    ix.quizzes.retain(|_, quiz| quiz.thread_id != thread_id);
    ix.awaiting_text.remove(&(chat_id, topic_id));
    tracing::info!(
        %thread_id,
        cleared = before - ix.callbacks.len(),
        "cleared Telegram interactions"
    );
}

fn interaction_token() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn thread_owner(state: &ServerState, thread_id: Uuid) -> Option<Uuid> {
    threads::select_cols((threads::owner,))
        .where_(threads::id.eq(thread_id))
        .all(&state.db)
        .await
        .ok()?
        .into_iter()
        .next()
        .map(|(owner,)| owner)
}

/// Registers the webhook with Telegram, enabling `callback_query` updates so inline button taps are
/// delivered. Without this (or if `allowed_updates` omits `callback_query`) Telegram silently drops
/// button presses, which is why approvals and quizzes never resolve.
pub(crate) async fn register_webhook(token: String, url: String, secret: Option<String>) {
    let mut payload = json!({
        "url": url,
        "allowed_updates": ["message", "callback_query"],
    });
    if let Some(secret) = secret {
        payload["secret_token"] = json!(secret);
    }
    let body = match serde_json::to_vec(&payload) {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(%error, "failed to encode setWebhook payload");
            return;
        }
    };

    let uri = format!("https://api.telegram.org/bot{token}/setWebhook");
    let req = match Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
    {
        Ok(req) => req,
        Err(error) => {
            tracing::warn!(%error, "failed to build setWebhook request");
            return;
        }
    };

    match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok((status, _))) if (200..300).contains(&status) => {
            tracing::info!(%url, "registered Telegram webhook with callback_query updates")
        }
        Ok(Ok((status, body))) => tracing::warn!(
            status,
            body = %String::from_utf8_lossy(&body),
            "setWebhook returned error"
        ),
        Ok(Err(error)) => tracing::warn!(%error, "failed to call setWebhook"),
        Err(error) => tracing::warn!(%error, "timed out calling setWebhook"),
    }
}

async fn telegram_post(state: &ServerState, method: &str, body: Vec<u8>) -> Option<Bytes> {
    let token = bot_token(state)?;
    let uri = format!("https://api.telegram.org/bot{token}/{method}");
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .ok()?;

    let (status, body) = match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, method, "failed to call Telegram API");
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, method, "timed out calling Telegram API");
            return None;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(
            method,
            status,
            body = %String::from_utf8_lossy(&body),
            "Telegram API returned error"
        );
        return None;
    }
    Some(body)
}

async fn send_telegram_buttons(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
    text: &str,
    keyboard: Vec<Vec<InlineButton>>,
) -> Option<TelegramSentMessage> {
    let text: String = text.chars().take(4096).collect();
    let (message_thread_id, direct_messages_topic_id) = topic_request_fields(topic_id);
    let request = SendButtonsRequest {
        chat_id,
        message_thread_id,
        direct_messages_topic_id,
        text: &text,
        reply_markup: ReplyKeyboardMarkup::from_inline_buttons(keyboard),
    };
    let body = telegram_post(state, "sendMessage", serde_json::to_vec(&request).ok()?).await?;
    serde_json::from_slice::<TelegramApiResponse<TelegramSendMessageResult>>(&body)
        .ok()
        .and_then(|response| response.result)
        .map(|message| TelegramSentMessage {
            chat_id: message.chat.id,
            message_id: message.message_id,
        })
}

async fn remove_telegram_keyboard(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
) -> Option<TelegramSentMessage> {
    let token = bot_token(state)?;

    let (message_thread_id, direct_messages_topic_id) = topic_request_fields(topic_id);
    let request = RemoveKeyboardRequest {
        chat_id,
        message_thread_id,
        direct_messages_topic_id,
        text: "Got it.",
        reply_markup: ReplyKeyboardRemove {
            remove_keyboard: true,
        },
    };
    let Ok(body) = serde_json::to_vec(&request) else {
        return None;
    };
    let uri = format!("https://api.telegram.org/bot{token}/sendMessage");
    let Ok(req) = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
    else {
        return None;
    };

    let (status, body) = match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to remove Telegram reply keyboard");
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, "timed out removing Telegram reply keyboard");
            return None;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(
            status,
            body = %String::from_utf8_lossy(&body),
            "Telegram reply keyboard removal returned error"
        );
        return None;
    }

    serde_json::from_slice::<TelegramApiResponse<TelegramSendMessageResult>>(&body)
        .ok()
        .and_then(|response| response.result)
        .map(|message| TelegramSentMessage {
            chat_id: message.chat.id,
            message_id: message.message_id,
        })
}

async fn answer_callback_query(state: &ServerState, callback_query_id: &str, text: &str) {
    let text: String = text.chars().take(200).collect();
    let Ok(body) = serde_json::to_vec(&json!({
        "callback_query_id": callback_query_id,
        "text": text,
    })) else {
        return;
    };
    let _ = telegram_post(state, "answerCallbackQuery", body).await;
}

async fn edit_telegram_message(state: &ServerState, chat_id: i64, message_id: i64, text: &str) {
    let text: String = text.chars().take(4096).collect();
    let Ok(body) = serde_json::to_vec(&json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": text,
    })) else {
        return;
    };
    let _ = telegram_post(state, "editMessageText", body).await;
}

async fn reply_thread(
    state: &ServerState,
    user_id: Uuid,
    message: &TelegramMessage,
) -> Result<Option<Uuid>, TelegramApiError> {
    let Some(reply) = message.reply_to_message.as_ref() else {
        return Ok(None);
    };

    let rows = telegram_message_links::select_cols((telegram_message_links::thread_id,))
        .where_(
            telegram_message_links::user_id
                .eq(user_id)
                .and(telegram_message_links::chat_id.eq(message.chat.id))
                .and(telegram_message_links::message_id.eq(reply.message_id)),
        )
        .all(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(rows.into_iter().next().map(|(thread_id,)| thread_id))
}

/// Returns the thread and whether it was just created.
async fn ensure_telegram_thread(
    state: &ServerState,
    user_id: Uuid,
    message: &TelegramMessage,
) -> Result<(Uuid, bool), TelegramApiError> {
    let topic_id = message.storage_topic_id();
    let rows = telegram_threads::select_cols((telegram_threads::thread_id,))
        .where_(
            telegram_threads::user_id
                .eq(user_id)
                .and(telegram_threads::chat_id.eq(message.chat.id))
                .and(telegram_threads::topic_id.eq(topic_id)),
        )
        .all(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    if let Some((thread_id,)) = rows.into_iter().next() {
        return Ok((thread_id, false));
    }

    if let Some(legacy_topic_id) = message.legacy_topic_id().filter(|id| *id != topic_id) {
        let rows = telegram_threads::select_cols((telegram_threads::thread_id,))
            .where_(
                telegram_threads::user_id
                    .eq(user_id)
                    .and(telegram_threads::chat_id.eq(message.chat.id))
                    .and(telegram_threads::topic_id.eq(legacy_topic_id)),
            )
            .all(&state.db)
            .await
            .map_err(|_| TelegramApiError::Internal)?;

        if let Some((thread_id,)) = rows.into_iter().next() {
            if let Err(error) = state
                .db
                .query_with_params(
                    "UPDATE telegram_threads SET topic_id = ? WHERE thread_id = ?",
                    vec![
                        minisql::Value::Integer(topic_id),
                        minisql::Value::Uuid(thread_id),
                    ],
                )
                .await
            {
                tracing::warn!(
                    %thread_id,
                    old_topic_id = legacy_topic_id,
                    new_topic_id = topic_id,
                    %error,
                    "failed to update legacy Telegram direct topic mapping"
                );
            }
            return Ok((thread_id, false));
        }
    }

    let thread_id = Uuid::now_v7();
    threads::insert()
        .id(thread_id)
        .owner(user_id)
        .title(DEFAULT_THREAD_TITLE)
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    telegram_threads::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .chat_id(message.chat.id)
        .topic_id(topic_id)
        .thread_id(thread_id)
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok((thread_id, true))
}

async fn finalize_latest_telegram_response(
    state: &ServerState,
    user_id: Uuid,
    thread_id: Uuid,
    chat_id: i64,
    message_thread_id: Option<i64>,
    content: &mut String,
) {
    if content.trim().is_empty()
        && let Some(latest_content) = latest_agent_message_content(state, thread_id).await
    {
        *content = latest_content;
    }
    finalize_telegram_stream(
        state,
        user_id,
        thread_id,
        chat_id,
        message_thread_id,
        content,
    )
    .await;
}

async fn finalize_telegram_stream(
    state: &ServerState,
    user_id: Uuid,
    thread_id: Uuid,
    chat_id: i64,
    message_thread_id: Option<i64>,
    content: &str,
) {
    let text = content.trim();
    if text.is_empty() {
        tracing::warn!(%thread_id, "Telegram response content is empty");
        return;
    };

    // Only resend as plain text when the rich send genuinely failed to reach Telegram. A delivered
    // message whose response we couldn't parse must not be resent, or it shows up twice.
    let sent = match send_telegram_rich_message(state, chat_id, message_thread_id, text).await {
        RichSend::Sent(message) => message,
        RichSend::Failed => send_telegram_message(state, chat_id, message_thread_id, text).await,
    };
    if let Some(message) = sent {
        let _ = link_telegram_message(
            state,
            user_id,
            message.chat_id,
            message.message_id,
            thread_id,
        )
        .await;
    }
}

async fn agent_message_content(state: &ServerState, message_id: Uuid) -> Option<String> {
    let rows = messages::select_cols((messages::content, messages::tool_calls))
        .where_(messages::id.eq(message_id))
        .all(&state.db)
        .await
        .ok()?;
    let (content, tool_calls) = rows.into_iter().next()?;
    if content.trim().is_empty() || tool_calls.is_some() {
        None
    } else {
        Some(content)
    }
}

async fn latest_agent_message_content(state: &ServerState, thread_id: Uuid) -> Option<String> {
    let rows = messages::select_cols((messages::content, messages::tool_calls))
        .where_(
            messages::parent_thread
                .eq(thread_id)
                .and(messages::role.eq(Role::Agent)),
        )
        .order_by_desc(messages::seq)
        .all(&state.db)
        .await
        .ok()?;

    rows.into_iter().find_map(|(content, tool_calls)| {
        if content.trim().is_empty() || tool_calls.is_some() {
            None
        } else {
            Some(content)
        }
    })
}

async fn send_telegram_rich_message_draft(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
    draft_id: i64,
    text: &str,
) -> bool {
    let Some(token) = bot_token(state) else {
        return false;
    };

    let markdown = rich_markdown(text);
    let (message_thread_id, direct_messages_topic_id) = topic_request_fields(topic_id);
    let request = SendRichMessageDraftRequest {
        chat_id,
        message_thread_id,
        direct_messages_topic_id,
        draft_id,
        rich_message: InputRichMessage {
            markdown: &markdown,
        },
    };
    let Ok(body) = serde_json::to_vec(&request) else {
        return false;
    };
    let uri = format!("https://api.telegram.org/bot{token}/sendRichMessageDraft");
    let Ok(req) = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
    else {
        return false;
    };

    let (status, body) = match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to send Telegram rich message draft");
            return false;
        }
        Err(error) => {
            tracing::warn!(%error, "timed out sending Telegram rich message draft");
            return false;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(
            status,
            body = %String::from_utf8_lossy(&body),
            "Telegram sendRichMessageDraft returned error"
        );
        return false;
    }

    true
}

/// Outcome of a rich (Markdown) Telegram send.
enum RichSend {
    /// Telegram accepted the message. The id is present only when the response could be parsed;
    /// either way the message was delivered, so the caller must not resend it as plain text.
    Sent(Option<TelegramSentMessage>),
    /// The request never reached Telegram (serialization, network, timeout, or API rejection), so
    /// resending as plain text is safe.
    Failed,
}

fn rich_send_outcome(status: u16, body: &[u8]) -> RichSend {
    if !(200..300).contains(&status) {
        return RichSend::Failed;
    }
    RichSend::Sent(
        serde_json::from_slice::<TelegramApiResponse<TelegramSendMessageResult>>(body)
            .ok()
            .and_then(|response| response.result)
            .map(|message| TelegramSentMessage {
                chat_id: message.chat.id,
                message_id: message.message_id,
            }),
    )
}

async fn send_telegram_rich_message(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
    text: &str,
) -> RichSend {
    let mut last = RichSend::Failed;
    for (index, chunk) in split_message(text, TELEGRAM_RICH_MESSAGE_LIMIT)
        .into_iter()
        .enumerate()
    {
        let sent = send_telegram_rich_message_chunk(state, chat_id, topic_id, &chunk).await;
        // If the very first chunk fails, report failure so the caller falls back to plain text for
        // the whole message rather than dropping it silently.
        if index == 0 && matches!(sent, RichSend::Failed) {
            return RichSend::Failed;
        }
        last = sent;
    }
    last
}

async fn send_telegram_rich_message_chunk(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
    text: &str,
) -> RichSend {
    let Some(token) = bot_token(state) else {
        return RichSend::Failed;
    };

    let markdown = rich_markdown(text);
    let (message_thread_id, direct_messages_topic_id) = topic_request_fields(topic_id);
    let request = SendRichMessageRequest {
        chat_id,
        message_thread_id,
        direct_messages_topic_id,
        rich_message: InputRichMessage {
            markdown: &markdown,
        },
    };
    let Ok(body) = serde_json::to_vec(&request) else {
        return RichSend::Failed;
    };
    let uri = format!("https://api.telegram.org/bot{token}/sendRichMessage");
    let Ok(req) = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
    else {
        return RichSend::Failed;
    };

    let (status, body) = match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to send Telegram rich message");
            return RichSend::Failed;
        }
        Err(error) => {
            tracing::warn!(%error, "timed out sending Telegram rich message");
            return RichSend::Failed;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(
            status,
            body = %String::from_utf8_lossy(&body),
            "Telegram sendRichMessage returned error"
        );
    }

    rich_send_outcome(status, &body)
}

pub(crate) async fn send_telegram_message(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
    text: &str,
) -> Option<TelegramSentMessage> {
    let mut last_sent = None;
    for chunk in split_message(text, TELEGRAM_MESSAGE_LIMIT) {
        last_sent = Some(send_telegram_message_chunk(state, chat_id, topic_id, &chunk).await?);
    }
    last_sent
}

async fn send_telegram_message_chunk(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
    text: &str,
) -> Option<TelegramSentMessage> {
    let token = bot_token(state)?;

    let (message_thread_id, direct_messages_topic_id) = topic_request_fields(topic_id);
    let request = SendMessageRequest {
        chat_id,
        message_thread_id,
        direct_messages_topic_id,
        text,
    };
    let Ok(body) = serde_json::to_vec(&request) else {
        return None;
    };
    let uri = format!("https://api.telegram.org/bot{token}/sendMessage");
    let Ok(req) = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
    else {
        return None;
    };

    let (status, body) = match timeout(Duration::from_secs(30), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to send Telegram message");
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, "timed out sending Telegram message");
            return None;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(
            status,
            body = %String::from_utf8_lossy(&body),
            "Telegram sendMessage returned error"
        );
        return None;
    }

    serde_json::from_slice::<TelegramApiResponse<TelegramSendMessageResult>>(&body)
        .ok()
        .and_then(|response| response.result)
        .map(|message| TelegramSentMessage {
            chat_id: message.chat.id,
            message_id: message.message_id,
        })
}

pub(crate) async fn link_telegram_message(
    state: &ServerState,
    user_id: Uuid,
    chat_id: i64,
    message_id: i64,
    thread_id: Uuid,
) -> Result<(), TelegramApiError> {
    let _ = telegram_message_links::delete()
        .where_(
            telegram_message_links::user_id
                .eq(user_id)
                .and(telegram_message_links::chat_id.eq(chat_id))
                .and(telegram_message_links::message_id.eq(message_id)),
        )
        .execute(&state.db)
        .await;

    telegram_message_links::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .chat_id(chat_id)
        .message_id(message_id)
        .thread_id(thread_id)
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(())
}

async fn connection_for_user(
    state: &ServerState,
    user_id: Uuid,
) -> Result<Option<TelegramConnection>, TelegramApiError> {
    let rows = telegram_connections::select()
        .where_(telegram_connections::user_id.eq(user_id))
        .all(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(rows.into_iter().next().map(|row| TelegramConnection {
        username: row.username,
        first_name: row.first_name,
        last_name: row.last_name,
    }))
}

async fn user_for_telegram_id(
    state: &ServerState,
    telegram_user_id: i64,
) -> Result<Option<Uuid>, TelegramApiError> {
    let rows = telegram_connections::select_cols((telegram_connections::user_id,))
        .where_(telegram_connections::telegram_user_id.eq(telegram_user_id))
        .all(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(rows.into_iter().next().map(|(id,)| id))
}

fn validate_secret(state: &ServerState, headers: &HeaderMap) -> Result<(), TelegramApiError> {
    let expected = state
        .config
        .server
        .as_ref()
        .and_then(|s| s.telegram.as_ref())
        .and_then(|t| t.read_webhook_secret())
        .ok_or(TelegramApiError::Unauthorized)?;

    let actual = headers
        .get(TELEGRAM_SECRET_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(TelegramApiError::Unauthorized)?;

    if crate::triggers::webhook::verify_secret(&expected, actual) {
        Ok(())
    } else {
        Err(TelegramApiError::Unauthorized)
    }
}

fn bot_token(state: &ServerState) -> Option<String> {
    state
        .config
        .server
        .as_ref()
        .and_then(|s| s.telegram.as_ref())
        .and_then(|t| t.read_bot_api_key())
        .filter(|t| !t.is_empty())
}

/// Verifies a Telegram Login Widget payload against the bot token.
///
/// Per https://core.telegram.org/bots/telegram-login the data is authentic when
/// `HMAC_SHA256(data_check_string, SHA256(bot_token))` equals the received `hash`,
/// where `data_check_string` is every field except `hash` formatted as `key=value`
/// and joined by newlines in alphabetical key order.
fn verify_login(token: &str, fields: &serde_json::Map<String, Value>) -> bool {
    let Some(hash) = fields.get("hash").and_then(Value::as_str) else {
        return false;
    };
    let Ok(provided) = hex::decode(hash) else {
        return false;
    };

    let mut pairs: Vec<(&str, String)> = fields
        .iter()
        .filter(|(key, _)| key.as_str() != "hash")
        .filter_map(|(key, value)| field_value(value).map(|v| (key.as_str(), v)))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let data_check_string = pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");

    let secret = Sha256::digest(token.as_bytes());
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&secret) else {
        return false;
    };
    mac.update(data_check_string.as_bytes());
    mac.verify_slice(&provided).is_ok()
}

fn field_value(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn is_start_command(text: &str) -> bool {
    let mut parts = text.split_whitespace();
    let Some(command) = parts.next() else {
        return false;
    };
    let command = command.split('@').next().unwrap_or(command);
    let is_start =
        command.eq_ignore_ascii_case("/connect") || command.eq_ignore_ascii_case("/start");
    is_start && parts.next().is_none()
}

async fn telegram_bot_username(state: &ServerState) -> Option<String> {
    if let Some(username) = configured_bot_username(state) {
        return Some(username);
    }

    let token = bot_token(state)?;
    let uri = format!("https://api.telegram.org/bot{token}/getMe");
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .ok()?;
    let (status, body) = timeout(Duration::from_secs(2), tinynet::send_request(req))
        .await
        .ok()?
        .ok()?;
    if !(200..300).contains(&status) {
        return None;
    }

    serde_json::from_slice::<TelegramApiResponse<TelegramGetMeResult>>(&body)
        .ok()
        .and_then(|response| response.result)
        .and_then(|user| user.username)
}

fn configured_bot_username(state: &ServerState) -> Option<String> {
    state
        .config
        .server
        .as_ref()
        .and_then(|s| s.telegram.as_ref())
        .and_then(|t| t.read_bot_username())
        .map(|username| username.trim_start_matches('@').to_string())
        .filter(|username| !username.is_empty())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs() as i64
}

pub(crate) async fn edit_forum_topic(
    state: &ServerState,
    chat_id: i64,
    message_thread_id: i64,
    name: &str,
) {
    let Ok(body) = serde_json::to_vec(&json!({
        "chat_id": chat_id,
        "message_thread_id": message_thread_id,
        "name": name,
    })) else {
        return;
    };
    let _ = telegram_post(state, "editForumTopic", body).await;
}

fn telegram_draft_id(run_id: Uuid) -> i64 {
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&run_id.as_bytes()[8..]);
    (i64::from_be_bytes(bytes) & i64::MAX).max(1)
}

fn rich_markdown(text: &str) -> String {
    text.chars().take(TELEGRAM_RICH_MESSAGE_LIMIT).collect()
}

fn telegram_draft_markdown(text: &str, thinking: &str) -> String {
    let text = rich_markdown(text.trim());
    let thinking = thinking.trim();
    let thinking = if thinking.is_empty() {
        "Thinking..."
    } else {
        thinking
    };
    let thinking = escape_telegram_rich_html(thinking);
    if text.is_empty() {
        format!("<tg-thinking>{thinking}</tg-thinking>")
    } else {
        format!("<tg-thinking>{thinking}</tg-thinking>\n\n{text}")
    }
}

fn escape_telegram_rich_html(text: &str) -> String {
    text.chars()
        .take(4096)
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            _ => vec![c],
        })
        .collect()
}

#[derive(Debug)]
struct TelegramConnection {
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramUpdate {
    message: Option<TelegramMessage>,
    business_message: Option<TelegramMessage>,
    guest_message: Option<TelegramMessage>,
    callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    id: String,
    from: TelegramUser,
    message: Option<CallbackMessage>,
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackMessage {
    message_id: i64,
    chat: TelegramChat,
    text: Option<String>,
}

impl TelegramUpdate {
    fn message(self) -> Option<TelegramMessage> {
        self.message
            .or(self.business_message)
            .or(self.guest_message)
    }
}

#[derive(Debug, Deserialize)]
pub struct TelegramMessage {
    message_id: i64,
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
    caption: Option<String>,
    message_thread_id: Option<i64>,
    direct_messages_topic: Option<DirectMessagesTopic>,
    reply_to_message: Option<TelegramReplyMessage>,
    forum_topic_created: Option<ForumTopicCreated>,
    document: Option<TelegramDocument>,
    photo: Option<Vec<TelegramPhotoSize>>,
    voice: Option<TelegramVoice>,
    audio: Option<TelegramAudio>,
    video: Option<TelegramVideo>,
    video_note: Option<TelegramVideoNote>,
}

/// A downloadable file attached to an incoming message, normalized across the
/// various Telegram media kinds (document, photo, audio, ...). Voice notes are
/// handled separately as [`VoiceNote`]: they are spoken messages, not files.
struct IncomingAttachment {
    file_id: String,
    file_name: String,
    mime_type: Option<String>,
}

/// A Telegram voice note. Unlike other media it is the message itself, so it is
/// transcribed up front and the transcript used as the user's text, rather than
/// downloaded and exposed to the agent as a file.
struct VoiceNote {
    file_id: String,
    mime_type: String,
}

impl TelegramMessage {
    /// The message's free-text, taken from `text` for plain messages and from
    /// `caption` for media messages.
    fn message_text(&self) -> Option<&str> {
        self.text
            .as_deref()
            .or(self.caption.as_deref())
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    /// The voice note attached to this message, if any. A voice note is a spoken
    /// message: it is transcribed before the agent runs, never stored as a file.
    fn voice_note(&self) -> Option<VoiceNote> {
        self.voice.as_ref().map(|voice| VoiceNote {
            file_id: voice.file_id.clone(),
            mime_type: voice
                .mime_type
                .clone()
                .unwrap_or_else(|| "audio/ogg".to_string()),
        })
    }

    /// Files attached to this message, with a sanitized workspace file name and
    /// best-known MIME type for each. Voice notes are excluded; see [`voice_note`].
    fn attachments(&self) -> Vec<IncomingAttachment> {
        let mut out = Vec::new();
        if let Some(doc) = &self.document {
            out.push(IncomingAttachment {
                file_id: doc.file_id.clone(),
                file_name: attachment_file_name(
                    doc.file_name.as_deref(),
                    &doc.file_unique_id,
                    doc.mime_type.as_deref(),
                    "file",
                ),
                mime_type: doc.mime_type.clone(),
            });
        }
        if let Some(largest) = self
            .photo
            .as_ref()
            .and_then(|sizes| sizes.iter().max_by_key(|s| s.file_size.unwrap_or(0)))
        {
            out.push(IncomingAttachment {
                file_id: largest.file_id.clone(),
                file_name: format!("photo_{}.jpg", largest.file_unique_id),
                mime_type: Some("image/jpeg".to_string()),
            });
        }
        if let Some(audio) = &self.audio {
            out.push(IncomingAttachment {
                file_id: audio.file_id.clone(),
                file_name: attachment_file_name(
                    audio.file_name.as_deref(),
                    &audio.file_unique_id,
                    audio.mime_type.as_deref(),
                    "audio",
                ),
                mime_type: audio.mime_type.clone(),
            });
        }
        if let Some(video) = &self.video {
            out.push(IncomingAttachment {
                file_id: video.file_id.clone(),
                file_name: attachment_file_name(
                    video.file_name.as_deref(),
                    &video.file_unique_id,
                    video.mime_type.as_deref(),
                    "video",
                ),
                mime_type: video.mime_type.clone(),
            });
        }
        if let Some(note) = &self.video_note {
            out.push(IncomingAttachment {
                file_id: note.file_id.clone(),
                file_name: format!("video_note_{}.mp4", note.file_unique_id),
                mime_type: Some("video/mp4".to_string()),
            });
        }
        out
    }

    fn send_topic_id(&self) -> Option<i64> {
        self.message_thread_id
            .map(encode_forum_topic_id)
            .or_else(|| self.legacy_topic_id().map(encode_direct_topic_id))
    }

    fn storage_topic_id(&self) -> i64 {
        self.send_topic_id().unwrap_or(0)
    }

    fn legacy_topic_id(&self) -> Option<i64> {
        self.direct_messages_topic
            .as_ref()
            .map(|topic| topic.topic_id)
    }
}

fn encode_forum_topic_id(topic_id: i64) -> i64 {
    topic_id
}

fn encode_direct_topic_id(topic_id: i64) -> i64 {
    -topic_id.abs()
}

fn topic_request_fields(topic_id: Option<i64>) -> (Option<i64>, Option<i64>) {
    match topic_id {
        Some(topic_id) if topic_id > 0 => (Some(topic_id), None),
        Some(topic_id) if topic_id < 0 => (None, Some(-topic_id)),
        _ => (None, None),
    }
}

#[derive(Debug, Deserialize)]
pub struct DirectMessagesTopic {
    topic_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct TelegramReplyMessage {
    message_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
pub struct TelegramUser {
    id: i64,
}

#[derive(Debug, Deserialize)]
pub struct ForumTopicCreated {}

#[derive(Debug, Deserialize)]
pub struct TelegramDocument {
    file_id: String,
    file_unique_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramPhotoSize {
    file_id: String,
    file_unique_id: String,
    file_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramVoice {
    file_id: String,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramAudio {
    file_id: String,
    file_unique_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramVideo {
    file_id: String,
    file_unique_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramVideoNote {
    file_id: String,
    file_unique_id: String,
}

/// Picks a safe workspace file name for an incoming attachment: the basename of
/// the Telegram-provided name when present, otherwise `<fallback>_<unique_id>`
/// with an extension guessed from the MIME type.
fn attachment_file_name(
    provided: Option<&str>,
    file_unique_id: &str,
    mime_type: Option<&str>,
    fallback: &str,
) -> String {
    if let Some(name) = provided.map(sanitize_file_name).filter(|n| !n.is_empty()) {
        return name;
    }
    match mime_extension(mime_type) {
        Some(ext) => format!("{fallback}_{file_unique_id}.{ext}"),
        None => format!("{fallback}_{file_unique_id}"),
    }
}

/// Reduces a path-like name to a single safe file name component.
fn sanitize_file_name(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect::<String>()
        .trim()
        .to_string()
}

fn mime_extension(mime_type: Option<&str>) -> Option<&'static str> {
    match mime_type?.split(';').next()?.trim() {
        "application/pdf" => Some("pdf"),
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "text/plain" => Some("txt"),
        "text/csv" => Some("csv"),
        "application/zip" => Some("zip"),
        "application/json" => Some("json"),
        "audio/ogg" => Some("ogg"),
        "audio/mpeg" => Some("mp3"),
        "video/mp4" => Some("mp4"),
        _ => None,
    }
}

#[derive(Serialize)]
struct SendMessageRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_messages_topic_id: Option<i64>,
    text: &'a str,
}

#[derive(Serialize)]
struct SendRichMessageDraftRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_messages_topic_id: Option<i64>,
    draft_id: i64,
    rich_message: InputRichMessage<'a>,
}

#[derive(Serialize)]
struct SendRichMessageRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_messages_topic_id: Option<i64>,
    rich_message: InputRichMessage<'a>,
}

#[derive(Serialize)]
struct InputRichMessage<'a> {
    markdown: &'a str,
}

#[derive(Serialize)]
struct SendButtonsRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_messages_topic_id: Option<i64>,
    text: &'a str,
    reply_markup: ReplyKeyboardMarkup,
}

#[derive(Serialize)]
struct RemoveKeyboardRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_messages_topic_id: Option<i64>,
    text: &'a str,
    reply_markup: ReplyKeyboardRemove,
}

#[derive(Serialize)]
struct InlineButton {
    text: String,
    callback_data: String,
}

#[derive(Serialize)]
struct ReplyKeyboardMarkup {
    keyboard: Vec<Vec<KeyboardButton>>,
    resize_keyboard: bool,
    one_time_keyboard: bool,
}

impl ReplyKeyboardMarkup {
    fn from_inline_buttons(buttons: Vec<Vec<InlineButton>>) -> Self {
        Self {
            keyboard: buttons
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|button| KeyboardButton { text: button.text })
                        .collect()
                })
                .collect(),
            resize_keyboard: true,
            one_time_keyboard: true,
        }
    }
}

#[derive(Serialize)]
struct KeyboardButton {
    text: String,
}

#[derive(Serialize)]
struct ReplyKeyboardRemove {
    remove_keyboard: bool,
}

#[derive(Deserialize)]
struct TelegramApiResponse<T> {
    result: Option<T>,
}

#[derive(Deserialize)]
struct TelegramSendMessageResult {
    message_id: i64,
    chat: TelegramSendMessageChat,
}

#[derive(Deserialize)]
struct TelegramSendMessageChat {
    id: i64,
}

#[derive(Deserialize)]
struct TelegramGetMeResult {
    username: Option<String>,
}

#[derive(Deserialize)]
struct TelegramFile {
    file_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn document_message_yields_named_attachment() {
        let update: TelegramUpdate = serde_json::from_str(
            r#"{"message":{"message_id":1,"chat":{"id":1,"type":"private"},"from":{"id":2},"caption":"see this","document":{"file_id":"FID","file_unique_id":"U","file_name":"report.pdf","mime_type":"application/pdf"}}}"#,
        )
        .unwrap();
        let message = update.message().unwrap();
        assert_eq!(message.message_text(), Some("see this"));
        let attachments = message.attachments();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].file_id, "FID");
        assert_eq!(attachments[0].file_name, "report.pdf");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn photo_message_without_caption_picks_largest_size() {
        let update: TelegramUpdate = serde_json::from_str(
            r#"{"message":{"message_id":1,"chat":{"id":1,"type":"private"},"from":{"id":2},"photo":[{"file_id":"small","file_unique_id":"s","file_size":100},{"file_id":"big","file_unique_id":"b","file_size":900}]}}"#,
        )
        .unwrap();
        let message = update.message().unwrap();
        assert_eq!(message.message_text(), None);
        let attachments = message.attachments();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].file_id, "big");
        assert_eq!(attachments[0].file_name, "photo_b.jpg");
    }

    #[test]
    fn voice_message_is_a_spoken_message_not_a_file() {
        let update: TelegramUpdate = serde_json::from_str(
            r#"{"message":{"message_id":1,"chat":{"id":1,"type":"private"},"from":{"id":2},"voice":{"duration":3,"mime_type":"audio/ogg","file_id":"VID","file_unique_id":"VU","file_size":4096}}}"#,
        )
        .unwrap();
        let message = update.message().unwrap();
        // A voice note carries no typed text and must never surface as a downloadable file.
        assert_eq!(message.message_text(), None);
        assert!(message.attachments().is_empty());
        let voice = message.voice_note().expect("voice note recognized");
        assert_eq!(voice.file_id, "VID");
        assert_eq!(voice.mime_type, "audio/ogg");
    }

    #[test]
    fn voice_message_defaults_mime_when_absent() {
        let update: TelegramUpdate = serde_json::from_str(
            r#"{"message":{"message_id":1,"chat":{"id":1,"type":"private"},"from":{"id":2},"voice":{"file_id":"VID","file_unique_id":"VU"}}}"#,
        )
        .unwrap();
        let voice = update.message().unwrap().voice_note().unwrap();
        assert_eq!(voice.mime_type, "audio/ogg");
    }

    #[test]
    fn attachment_file_name_sanitizes_and_falls_back() {
        assert_eq!(
            attachment_file_name(None, "U", Some("application/pdf"), "file"),
            "file_U.pdf"
        );
        assert_eq!(attachment_file_name(None, "U", None, "audio"), "audio_U");
        assert_eq!(
            attachment_file_name(Some("../etc/passwd"), "U", None, "file"),
            "passwd"
        );
    }

    #[test]
    fn rich_send_delivered_is_not_resent_as_plain_text() {
        // 2xx with a parseable result: delivered, id captured.
        let ok = br#"{"result":{"message_id":7,"chat":{"id":42}}}"#;
        match rich_send_outcome(200, ok) {
            RichSend::Sent(Some(message)) => {
                assert_eq!(message.message_id, 7);
                assert_eq!(message.chat_id, 42);
            }
            _ => panic!("expected Sent with id"),
        }

        // 2xx but unparseable body: still delivered, so no plain-text fallback (no duplicate).
        match rich_send_outcome(200, b"not json") {
            RichSend::Sent(None) => {}
            _ => panic!("expected Sent without id"),
        }

        // Non-2xx: not delivered, plain-text fallback is allowed.
        assert!(matches!(rich_send_outcome(400, b""), RichSend::Failed));
    }

    #[test]
    fn telegram_draft_markdown_uses_thinking_block_only_for_drafts() {
        assert_eq!(
            telegram_draft_markdown("", ""),
            "<tg-thinking>Thinking...</tg-thinking>"
        );
        assert_eq!(
            telegram_draft_markdown("Hello", "Reading context"),
            "<tg-thinking>Reading context</tg-thinking>\n\nHello"
        );
        assert_eq!(
            telegram_draft_markdown("Hello", "A < B && C > D"),
            "<tg-thinking>A &lt; B &amp;&amp; C &gt; D</tg-thinking>\n\nHello"
        );
        assert_eq!(
            telegram_draft_markdown("Hello", ""),
            "<tg-thinking>Thinking...</tg-thinking>\n\nHello"
        );
    }

    #[test]
    fn start_command_only_matches_bare_start_commands() {
        assert!(is_start_command("/start"));
        assert!(is_start_command("/connect@stride_bot"));
        assert!(!is_start_command("hello"));
        assert!(!is_start_command("/start 123456"));
        assert!(!is_start_command("/connect 123456"));
    }

    #[test]
    fn login_verification_accepts_genuine_signature_and_rejects_tampering() {
        let token = "123456:test-bot-token";
        let mut fields = serde_json::Map::new();
        fields.insert("id".into(), Value::from(42_i64));
        fields.insert("first_name".into(), Value::from("Ada"));
        fields.insert("username".into(), Value::from("ada"));
        fields.insert("auth_date".into(), Value::from(1_700_000_000_i64));

        let hash = sign_login(token, &fields);
        fields.insert("hash".into(), Value::from(hash));
        assert!(verify_login(token, &fields));

        // Tampered field invalidates the signature.
        let mut tampered = fields.clone();
        tampered.insert("id".into(), Value::from(99_i64));
        assert!(!verify_login(token, &tampered));

        // Wrong bot token invalidates the signature.
        assert!(!verify_login("999999:other-token", &fields));
    }

    fn state_with_webhook_secret(secret: Option<&str>) -> ServerState {
        let telegram = crate::config::Telegram {
            bot_api_key: None,
            bot_username: None,
            webhook_secret: secret.map(str::to_owned),
            webhook_url: None,
        };
        let server = crate::config::Server {
            db_url: None,
            db_path: None,
            listen_addr: None,
            allow_registration: None,
            ldap: None,
            files: None,
            telegram: Some(telegram),
            github: None,
            google: None,
            public_url: None,
            agent: None,
        };
        ServerState {
            config: Config {
                providers: HashMap::new(),
                models: HashMap::new(),
                server: Some(server),
                tools: None,
                mcp: HashMap::new(),
            },
            db: ConnectionPool::new("sqlite::memory:").unwrap(),
            jwt_secret: String::new(),
            runner: Arc::new(FakePool::default()),
            model_config: Arc::new(stride_agent::AgentConfig {
                model_registry: stride_agent::ModelRegistry::default(),
                max_iterations: 1,
            }),
            vfs: None,
            telegram_interactions: Arc::new(Mutex::new(Interactions::default())),
            executor: crate::scheduler::ExecutorHandle::channel().0,
            cipher: crate::crypto::SecretCipher::new("test-secret"),
            google_service: None,
        }
    }

    fn headers_with_secret(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(TELEGRAM_SECRET_HEADER, value.parse().unwrap());
        headers
    }

    #[test]
    fn validate_secret_rejects_when_no_secret_configured() {
        let state = state_with_webhook_secret(None);
        assert!(matches!(
            validate_secret(&state, &HeaderMap::new()),
            Err(TelegramApiError::Unauthorized)
        ));
        assert!(matches!(
            validate_secret(&state, &headers_with_secret("anything")),
            Err(TelegramApiError::Unauthorized)
        ));
    }

    #[test]
    fn validate_secret_rejects_empty_configured_secret() {
        let state = state_with_webhook_secret(Some(""));
        assert!(matches!(
            validate_secret(&state, &headers_with_secret("")),
            Err(TelegramApiError::Unauthorized)
        ));
    }

    #[test]
    fn validate_secret_accepts_matching_header() {
        let state = state_with_webhook_secret(Some("s3cr3t"));
        assert!(validate_secret(&state, &headers_with_secret("s3cr3t")).is_ok());
    }

    #[test]
    fn validate_secret_rejects_wrong_or_missing_header() {
        let state = state_with_webhook_secret(Some("s3cr3t"));
        assert!(matches!(
            validate_secret(&state, &headers_with_secret("nope")),
            Err(TelegramApiError::Unauthorized)
        ));
        assert!(matches!(
            validate_secret(&state, &HeaderMap::new()),
            Err(TelegramApiError::Unauthorized)
        ));
    }

    fn sign_login(token: &str, fields: &serde_json::Map<String, Value>) -> String {
        let mut pairs: Vec<(&str, String)> = fields
            .iter()
            .filter(|(key, _)| key.as_str() != "hash")
            .filter_map(|(key, value)| field_value(value).map(|v| (key.as_str(), v)))
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        let data_check_string = pairs
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let secret = Sha256::digest(token.as_bytes());
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret).unwrap();
        mac.update(data_check_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    use async_trait::async_trait;
    use minisql::ConnectionPool;

    use crate::{
        config::Config,
        db::users,
        runner::{AgentEvent, AgentPool, AgentPoolError, RunId, ThreadSnapshot, ThreadStatus},
    };

    #[derive(Default)]
    struct FakePool {
        received: Mutex<Vec<String>>,
        approvals: Mutex<Vec<(Uuid, Uuid, bool)>>,
        quiz_answers: Mutex<Vec<(Uuid, Uuid, Vec<String>)>>,
    }

    #[async_trait]
    impl AgentPool for FakePool {
        async fn send(
            &self,
            thread_id: Uuid,
            request: AgentRequest,
        ) -> Result<RunId, AgentPoolError> {
            self.received.lock().unwrap().push(request.content);
            let run_id = RunId(Uuid::now_v7());
            pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).publish(AgentEvent {
                seq: 1,
                thread_id,
                run_id: Some(run_id),
                kind: AgentEventKind::RunStarted,
            });
            pubsub::topic::<AgentEvent>(&thread_events_topic(thread_id)).publish(AgentEvent {
                seq: 2,
                thread_id,
                run_id: Some(run_id),
                kind: AgentEventKind::RunFinished,
            });
            Ok(run_id)
        }

        async fn snapshot(&self, thread_id: Uuid) -> Result<ThreadSnapshot, AgentPoolError> {
            Ok(ThreadSnapshot {
                thread_id,
                last_event_seq: 0,
                status: ThreadStatus::Idle,
                in_progress: None,
                tool_progress: Vec::new(),
                pending_approval: None,
                pending_quiz: None,
            })
        }

        async fn status(&self, _thread_id: Uuid) -> Result<ThreadStatus, AgentPoolError> {
            Ok(ThreadStatus::Idle)
        }

        async fn cancel_run(&self, _thread_id: Uuid) -> Result<(), AgentPoolError> {
            Ok(())
        }

        async fn resolve_approval(
            &self,
            thread_id: Uuid,
            approval_id: Uuid,
            approved: bool,
        ) -> Result<(), AgentPoolError> {
            self.approvals
                .lock()
                .unwrap()
                .push((thread_id, approval_id, approved));
            Ok(())
        }

        async fn answer_quiz(
            &self,
            thread_id: Uuid,
            quiz_id: Uuid,
            answers: Vec<String>,
        ) -> Result<(), AgentPoolError> {
            self.quiz_answers
                .lock()
                .unwrap()
                .push((thread_id, quiz_id, answers));
            Ok(())
        }

        async fn shutdown_thread(&self, _thread_id: Uuid) -> Result<(), AgentPoolError> {
            Ok(())
        }
    }

    async fn build_state(pool: Arc<FakePool>) -> Arc<ServerState> {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(crate::db::get_migrations())
            .await
            .unwrap();
        Arc::new(ServerState {
            config: Config {
                providers: HashMap::new(),
                models: HashMap::new(),
                server: None,
                tools: None,
                mcp: HashMap::new(),
            },
            db,
            jwt_secret: String::new(),
            runner: pool,
            model_config: Arc::new(stride_agent::AgentConfig {
                model_registry: stride_agent::ModelRegistry::default(),
                max_iterations: 1,
            }),
            vfs: None,
            telegram_interactions: Arc::new(Mutex::new(Interactions::default())),
            executor: crate::scheduler::ExecutorHandle::channel().0,
            cipher: crate::crypto::SecretCipher::new("test-secret"),
            google_service: None,
        })
    }

    /// Seeds a user that owns `thread_id` and is linked to a Telegram account, so callback
    /// authorization (thread owner == caller) passes.
    async fn seed_owner(
        state: &ServerState,
        user_id: Uuid,
        thread_id: Uuid,
        telegram_user_id: i64,
        chat_id: i64,
    ) {
        users::insert()
            .id(user_id)
            .username(user_id.to_string().as_str())
            .password_hash("x")
            .personality(Option::<&str>::None)
            .execute(&state.db)
            .await
            .unwrap();
        threads::insert()
            .id(thread_id)
            .owner(user_id)
            .title("t")
            .execute(&state.db)
            .await
            .unwrap();
        telegram_connections::insert()
            .id(Uuid::now_v7())
            .user_id(user_id)
            .telegram_user_id(telegram_user_id)
            .chat_id(chat_id)
            .username(Option::<&str>::None)
            .first_name(Option::<&str>::None)
            .last_name(Option::<&str>::None)
            .connected_at(0)
            .execute(&state.db)
            .await
            .unwrap();
    }

    async fn seed_telegram_thread(
        state: &ServerState,
        user_id: Uuid,
        thread_id: Uuid,
        chat_id: i64,
        topic_id: i64,
    ) {
        telegram_threads::insert()
            .id(Uuid::now_v7())
            .user_id(user_id)
            .chat_id(chat_id)
            .topic_id(topic_id)
            .thread_id(thread_id)
            .execute(&state.db)
            .await
            .unwrap();
    }

    fn callback(token: String, from_id: i64, chat_id: i64, text: &str) -> CallbackQuery {
        CallbackQuery {
            id: "cb".to_string(),
            from: TelegramUser { id: from_id },
            message: Some(CallbackMessage {
                message_id: 10,
                chat: TelegramChat {
                    id: chat_id,
                    kind: "private".to_string(),
                },
                text: Some(text.to_string()),
            }),
            data: Some(token),
        }
    }

    fn text_message(text: &str, from_id: i64, chat_id: i64) -> TelegramMessage {
        TelegramMessage {
            message_id: 11,
            chat: TelegramChat {
                id: chat_id,
                kind: "private".to_string(),
            },
            from: Some(TelegramUser { id: from_id }),
            text: Some(text.to_string()),
            message_thread_id: None,
            direct_messages_topic: None,
            reply_to_message: None,
            forum_topic_created: None,
            caption: None,
            document: None,
            photo: None,
            voice: None,
            audio: None,
            video: None,
            video_note: None,
        }
    }

    fn find_token(state: &ServerState, predicate: impl Fn(&CallbackAction) -> bool) -> String {
        state
            .telegram_interactions
            .lock()
            .unwrap()
            .callbacks
            .iter()
            .find_map(|(token, action)| predicate(action).then(|| token.clone()))
            .expect("token not found")
    }

    #[tokio::test]
    async fn approval_button_resolves_run() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;
        let (user_id, thread_id, approval_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        present_approval(&state, thread_id, 42, None, approval_id, "Run shell?").await;
        let approve = find_token(&state, |action| {
            matches!(action, CallbackAction::Approval { approved: true, .. })
        });

        handle_callback(&state, callback(approve, 555, 42, "⚠️ Run shell?")).await;

        assert_eq!(
            *pool.approvals.lock().unwrap(),
            vec![(thread_id, approval_id, true)]
        );
        // Both approve and deny tokens are cleared after resolution.
        assert!(
            state
                .telegram_interactions
                .lock()
                .unwrap()
                .callbacks
                .is_empty()
        );
    }

    #[tokio::test]
    async fn approval_button_rejects_non_owner() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;
        let (user_id, thread_id, approval_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        present_approval(&state, thread_id, 42, None, approval_id, "Run shell?").await;
        let approve = find_token(&state, |action| {
            matches!(action, CallbackAction::Approval { approved: true, .. })
        });

        // A different Telegram user (not connected / not the owner) taps the button.
        handle_callback(&state, callback(approve, 999, 42, "⚠️ Run shell?")).await;

        assert!(pool.approvals.lock().unwrap().is_empty());
        assert!(
            !state
                .telegram_interactions
                .lock()
                .unwrap()
                .callbacks
                .is_empty(),
            "unauthorized taps must not consume callback tokens"
        );
    }

    #[tokio::test]
    async fn approval_reply_keyboard_text_resolves_run() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;
        let (user_id, thread_id, approval_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        present_approval(&state, thread_id, 42, None, approval_id, "Run shell?").await;
        let handled = resolve_text_action(&state, &text_message("Approve", 555, 42), "Approve")
            .await
            .unwrap();

        assert!(handled);
        assert_eq!(
            *pool.approvals.lock().unwrap(),
            vec![(thread_id, approval_id, true)]
        );
        assert!(
            state
                .telegram_interactions
                .lock()
                .unwrap()
                .text_actions
                .is_empty()
        );
    }

    #[tokio::test]
    async fn quiz_collects_option_answers_across_questions() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;
        let (user_id, thread_id, quiz_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        let questions = vec![
            QuizQuestion {
                question: "Pick A".to_string(),
                options: vec!["a1".to_string(), "a2".to_string()],
            },
            QuizQuestion {
                question: "Pick B".to_string(),
                options: vec!["b1".to_string(), "b2".to_string()],
            },
        ];
        present_quiz(&state, thread_id, 42, None, quiz_id, questions).await;

        let pick = |index: usize, answer: &str| {
            let answer = answer.to_string();
            move |action: &CallbackAction| {
                matches!(action, CallbackAction::QuizOption { question_index, answer: a, .. }
                    if *question_index == index && *a == answer)
            }
        };

        let a2 = find_token(&state, pick(0, "a2"));
        handle_callback(&state, callback(a2, 555, 42, "❓ (1/2) Pick A")).await;
        let b1 = find_token(&state, pick(1, "b1"));
        handle_callback(&state, callback(b1, 555, 42, "❓ (2/2) Pick B")).await;

        assert_eq!(
            *pool.quiz_answers.lock().unwrap(),
            vec![(thread_id, quiz_id, vec!["a2".to_string(), "b1".to_string()])]
        );
    }

    #[tokio::test]
    async fn quiz_free_form_answer_captured_from_text_reply() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;
        let (user_id, thread_id, quiz_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        let questions = vec![QuizQuestion {
            question: "Your name?".to_string(),
            options: vec![],
        }];
        present_quiz(&state, thread_id, 42, None, quiz_id, questions).await;

        // The free-form question routes the user's next typed message as the answer.
        assert_eq!(pending_free_form_quiz(&state, 42, None), Some((quiz_id, 0)));
        answer_quiz_question(&state, quiz_id, 0, "Alice".to_string()).await;

        assert_eq!(
            *pool.quiz_answers.lock().unwrap(),
            vec![(thread_id, quiz_id, vec!["Alice".to_string()])]
        );
        // Routing state is cleared once answered.
        assert_eq!(pending_free_form_quiz(&state, 42, None), None);
    }

    #[tokio::test]
    async fn dispatcher_quiz_button_tap_resolves_through_pool() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;
        let (user_id, thread_id) = (Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;
        seed_telegram_thread(&state, user_id, thread_id, 42, 0).await;

        let mut subscriber = TelegramSubscriber {
            state: state.clone(),
            thread_id,
            active: None,
        };
        let run_id = RunId(Uuid::now_v7());
        let event = |kind| AgentEvent {
            seq: 0,
            thread_id,
            run_id: Some(run_id),
            kind,
        };
        subscriber
            .handle_event(&event(AgentEventKind::RunStarted))
            .await;
        let quiz_id = Uuid::now_v7();
        subscriber
            .handle_event(&event(AgentEventKind::WaitingForQuiz {
                quiz_id,
                questions: vec![QuizQuestion {
                    question: "Pick".to_string(),
                    options: vec!["a".to_string(), "b".to_string()],
                }],
            }))
            .await;

        // Tap the "a" button as the thread owner, through the real webhook callback handler.
        let token = find_token(
            &state,
            |action| matches!(action, CallbackAction::QuizOption { answer, .. } if answer == "a"),
        );
        handle_callback(&state, callback(token, 555, 42, "❓ (1/1) Pick")).await;

        assert_eq!(
            *pool.quiz_answers.lock().unwrap(),
            vec![(thread_id, quiz_id, vec!["a".to_string()])]
        );
    }

    #[tokio::test]
    async fn connected_user_alone_is_not_a_telegram_destination() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool).await;
        let (user_id, thread_id) = (Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        assert!(telegram_destination(&state.db, thread_id).await.is_none());
        assert!(!thread_has_telegram_mapping(&state.db, thread_id).await);
    }

    #[tokio::test]
    async fn telegram_mapping_is_a_telegram_destination() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool).await;
        let (user_id, thread_id) = (Uuid::now_v7(), Uuid::now_v7());
        seed_owner(&state, user_id, thread_id, 555, 42).await;
        seed_telegram_thread(&state, user_id, thread_id, 42, 7).await;

        assert_eq!(
            telegram_destination(&state.db, thread_id).await,
            Some((42, Some(7), user_id))
        );
        assert!(thread_has_telegram_mapping(&state.db, thread_id).await);
    }

    #[test]
    fn forum_topic_buttons_use_message_thread_id() {
        let (message_thread_id, direct_messages_topic_id) = topic_request_fields(Some(7));
        let request = SendButtonsRequest {
            chat_id: 42,
            message_thread_id,
            direct_messages_topic_id,
            text: "Pick",
            reply_markup: ReplyKeyboardMarkup::from_inline_buttons(vec![vec![InlineButton {
                text: "A".to_string(),
                callback_data: "token".to_string(),
            }]]),
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["message_thread_id"], 7);
        assert!(value.get("direct_messages_topic_id").is_none());
        assert_eq!(value["reply_markup"]["keyboard"][0][0]["text"], "A");
        assert_eq!(value["reply_markup"]["one_time_keyboard"], true);
        assert!(value["reply_markup"].get("inline_keyboard").is_none());
    }

    #[test]
    fn direct_topic_buttons_use_direct_messages_topic_id() {
        let (message_thread_id, direct_messages_topic_id) = topic_request_fields(Some(-99));
        let request = SendButtonsRequest {
            chat_id: 42,
            message_thread_id,
            direct_messages_topic_id,
            text: "Pick",
            reply_markup: ReplyKeyboardMarkup::from_inline_buttons(Vec::new()),
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["direct_messages_topic_id"], 99);
        assert!(value.get("message_thread_id").is_none());
    }

    #[test]
    fn keyboard_removal_payload_targets_topic() {
        let (message_thread_id, direct_messages_topic_id) = topic_request_fields(Some(7));
        let request = RemoveKeyboardRequest {
            chat_id: 42,
            message_thread_id,
            direct_messages_topic_id,
            text: "Got it.",
            reply_markup: ReplyKeyboardRemove {
                remove_keyboard: true,
            },
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["message_thread_id"], 7);
        assert_eq!(value["reply_markup"]["remove_keyboard"], true);
        assert!(value.get("direct_messages_topic_id").is_none());
    }

    #[tokio::test]
    async fn private_telegram_message_creates_destination_mapping() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool).await;
        let user_id = Uuid::now_v7();
        let thread_id = Uuid::now_v7();
        seed_owner(&state, user_id, thread_id, 555, 42).await;

        let message = TelegramMessage {
            message_id: 12,
            chat: TelegramChat {
                id: 42,
                kind: "private".to_string(),
            },
            from: Some(TelegramUser { id: 555 }),
            text: Some("hello".to_string()),
            message_thread_id: None,
            direct_messages_topic: None,
            reply_to_message: None,
            forum_topic_created: None,
            caption: None,
            document: None,
            photo: None,
            voice: None,
            audio: None,
            video: None,
            video_note: None,
        };

        let (created_thread_id, is_new) = ensure_telegram_thread(&state, user_id, &message)
            .await
            .unwrap();

        assert!(is_new);
        assert_ne!(created_thread_id, thread_id);
        assert_eq!(
            telegram_destination(&state.db, created_thread_id).await,
            Some((42, None, user_id))
        );
    }

    #[tokio::test]
    async fn direct_telegram_topic_creates_encoded_destination_mapping() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool).await;
        let user_id = Uuid::now_v7();
        let message = TelegramMessage {
            message_id: 12,
            chat: TelegramChat {
                id: 42,
                kind: "private".to_string(),
            },
            from: Some(TelegramUser { id: 555 }),
            text: Some("hello".to_string()),
            message_thread_id: None,
            direct_messages_topic: Some(DirectMessagesTopic { topic_id: 99 }),
            reply_to_message: None,
            forum_topic_created: None,
            caption: None,
            document: None,
            photo: None,
            voice: None,
            audio: None,
            video: None,
            video_note: None,
        };

        let (created_thread_id, is_new) = ensure_telegram_thread(&state, user_id, &message)
            .await
            .unwrap();

        assert!(is_new);
        assert_eq!(
            telegram_destination(&state.db, created_thread_id).await,
            Some((42, Some(-99), user_id))
        );
    }
}
