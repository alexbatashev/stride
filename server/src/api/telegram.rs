use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use friday_agent::QuizQuestion;
use http_body_util::Full;
use hyper::Request;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    sync::{broadcast::error::RecvError, mpsc},
    time::{interval, sleep, timeout},
};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    api::threads::DEFAULT_THREAD_TITLE,
    db::{
        Role, messages, telegram_connect_codes, telegram_connections, telegram_message_links,
        telegram_threads, threads,
    },
    runner::{AgentEventKind, AgentRequest, ThreadStatus},
};

const CONNECT_CODE_TTL_SECONDS: i64 = 10 * 60;
const TELEGRAM_SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";
/// How long a per-thread session task lingers with an empty queue before retiring.
const SESSION_IDLE_TTL: Duration = Duration::from_secs(60);

#[derive(Serialize)]
pub struct TelegramSettingsResponse {
    bot_configured: bool,
    bot_username: Option<String>,
    connected: bool,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Serialize)]
pub struct TelegramConnectCodeResponse {
    code: String,
    expires_at: i64,
    start_url: Option<String>,
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
    let bot_username = configured_bot_username(&state);

    Ok(Json(TelegramSettingsResponse {
        bot_configured: bot_token(&state).is_some(),
        bot_username,
        connected: connection.is_some(),
        username: connection.as_ref().and_then(|c| c.username.clone()),
        first_name: connection.as_ref().and_then(|c| c.first_name.clone()),
        last_name: connection.and_then(|c| c.last_name),
    }))
}

pub async fn create_connect_code(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<TelegramConnectCodeResponse>, TelegramApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    if bot_token(&state).is_none() {
        return Err(TelegramApiError::NotFound);
    }

    let code = generate_code();
    let expires_at = now() + CONNECT_CODE_TTL_SECONDS;
    let start_url = telegram_bot_username(&state)
        .await
        .map(|username| format!("https://t.me/{username}?start={code}"));

    telegram_connect_codes::delete()
        .where_(telegram_connect_codes::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    telegram_connect_codes::insert()
        .code(code.as_str())
        .user_id(user_id)
        .expires_at(expires_at)
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(Json(TelegramConnectCodeResponse {
        code,
        expires_at,
        start_url,
    }))
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
    telegram_connect_codes::delete()
        .where_(telegram_connect_codes::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn webhook(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(mut update): Json<TelegramUpdate>,
) -> Result<StatusCode, TelegramApiError> {
    validate_secret(&state, &headers)?;

    if let Some(callback) = update.callback_query.take() {
        handle_callback(&state, callback).await;
        return Ok(StatusCode::OK);
    }

    let Some(message) = update.message() else {
        return Ok(StatusCode::OK);
    };

    if let Some(code) = connect_code(message.text.as_deref()) {
        handle_connect_command(&state, &message, code).await?;
        return Ok(StatusCode::OK);
    }

    handle_topic_message(state, message).await?;
    Ok(StatusCode::OK)
}

async fn handle_connect_command(
    state: &ServerState,
    message: &TelegramMessage,
    code: &str,
) -> Result<(), TelegramApiError> {
    let Some(from) = message.from.as_ref() else {
        return Ok(());
    };

    let rows = telegram_connect_codes::select_cols((telegram_connect_codes::user_id,))
        .where_(
            telegram_connect_codes::code
                .eq(code)
                .and(telegram_connect_codes::expires_at.gt(now())),
        )
        .all(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    let Some((user_id,)) = rows.into_iter().next() else {
        send_telegram_message(
            state,
            message.chat.id,
            message.topic_id(),
            "Invalid or expired connect code.",
        )
        .await;
        return Ok(());
    };

    telegram_connections::delete()
        .where_(
            telegram_connections::user_id
                .eq(user_id)
                .or(telegram_connections::telegram_user_id.eq(from.id)),
        )
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    telegram_connections::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .telegram_user_id(from.id)
        .chat_id(message.chat.id)
        .username(from.username.as_deref())
        .first_name(from.first_name.as_deref())
        .last_name(from.last_name.as_deref())
        .connected_at(now())
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    telegram_connect_codes::delete()
        .where_(telegram_connect_codes::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    send_telegram_message(
        state,
        message.chat.id,
        message.topic_id(),
        "Telegram connected to Friday.",
    )
    .await;
    Ok(())
}

async fn handle_topic_message(
    state: Arc<ServerState>,
    message: TelegramMessage,
) -> Result<(), TelegramApiError> {
    let Some(from) = message.from.as_ref() else {
        return Ok(());
    };

    let Some(user_id) = user_for_telegram_id(&state, from.id).await? else {
        if message.chat.kind == "private" {
            send_telegram_message(
                &state,
                message.chat.id,
                message.topic_id(),
                "Open Friday Settings and create a Telegram connect code first.",
            )
            .await;
        }
        return Ok(());
    };

    if message.forum_topic_created.is_some() {
        return Ok(());
    }

    let Some(text) = message
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return Ok(());
    };

    if text.starts_with('/') {
        return Ok(());
    }

    // A plain message answers a pending free-form quiz question instead of starting a new run.
    if let Some((quiz_id, question_index)) =
        pending_free_form_quiz(&state, message.chat.id, message.topic_id())
    {
        answer_quiz_question(&state, quiz_id, question_index, text.to_string()).await;
        return Ok(());
    }

    let (thread_id, is_new) =
        if let Some(thread_id) = reply_thread(&state, user_id, &message).await? {
            (thread_id, false)
        } else if message.topic_id().unwrap_or(0) == 0 {
            (create_telegram_thread(&state, user_id).await?, true)
        } else {
            ensure_telegram_thread(&state, user_id, &message).await?
        };

    if is_new {
        crate::api::threads::spawn_title_generation(
            state.clone(),
            thread_id,
            text.to_string(),
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

    let queued = QueuedMessage {
        text: text.to_string(),
        chat_id: message.chat.id,
        topic_id: message.topic_id(),
    };
    state
        .telegram_sessions
        .clone()
        .dispatch(state.clone(), thread_id, user_id, queued);

    Ok(())
}

struct QueuedMessage {
    text: String,
    chat_id: i64,
    topic_id: Option<i64>,
}

/// Per-thread Telegram sessions layered over the shared `AgentPool`.
///
/// Each thread gets one long-lived session task that owns a FIFO queue: it runs the agent for
/// one queued message at a time and forwards that run's events to Telegram. Queueing (instead of
/// sending straight to the pool) is what keeps concurrent Telegram messages from being dropped
/// with `AlreadyRunning`.
#[derive(Default)]
pub(crate) struct TelegramSessions {
    inner: Mutex<HashMap<Uuid, mpsc::UnboundedSender<QueuedMessage>>>,
    interactions: Mutex<Interactions>,
}

/// Pending interactive prompts (approvals and quizzes) shown in Telegram as inline buttons or, for
/// free-form quiz questions, captured from the user's next typed reply.
#[derive(Default)]
struct Interactions {
    /// Button `callback_data` token → the action that tap performs.
    callbacks: HashMap<String, CallbackAction>,
    /// In-flight quizzes, keyed by quiz id, collecting one answer per question.
    quizzes: HashMap<Uuid, QuizState>,
    /// (chat_id, topic_id) currently awaiting a typed free-form answer → quiz id.
    awaiting_text: HashMap<(i64, Option<i64>), Uuid>,
}

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

impl TelegramSessions {
    fn dispatch(
        self: Arc<Self>,
        state: Arc<ServerState>,
        thread_id: Uuid,
        user_id: Uuid,
        message: QueuedMessage,
    ) {
        let mut sessions = self.inner.lock().unwrap();
        if let Some(tx) = sessions.get(&thread_id) {
            match tx.send(message) {
                Ok(()) => return,
                // Session task is retiring; drop the stale sender and start a fresh one.
                Err(returned) => {
                    sessions.remove(&thread_id);
                    return self.spawn_session(sessions, state, thread_id, user_id, returned.0);
                }
            }
        }
        self.spawn_session(sessions, state, thread_id, user_id, message);
    }

    fn spawn_session(
        self: &Arc<Self>,
        mut sessions: std::sync::MutexGuard<
            '_,
            HashMap<Uuid, mpsc::UnboundedSender<QueuedMessage>>,
        >,
        state: Arc<ServerState>,
        thread_id: Uuid,
        user_id: Uuid,
        message: QueuedMessage,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(message);
        sessions.insert(thread_id, tx);
        drop(sessions);
        tokio::spawn(run_session(state, self.clone(), thread_id, user_id, rx));
    }
}

async fn run_session(
    state: Arc<ServerState>,
    sessions: Arc<TelegramSessions>,
    thread_id: Uuid,
    user_id: Uuid,
    mut rx: mpsc::UnboundedReceiver<QueuedMessage>,
) {
    loop {
        let message = match timeout(SESSION_IDLE_TTL, rx.recv()).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(_) => {
                // Idle: retire under the lock so a racing dispatch either lands here or recreates.
                let mut map = sessions.inner.lock().unwrap();
                match rx.try_recv() {
                    Ok(message) => {
                        drop(map);
                        message
                    }
                    Err(_) => {
                        map.remove(&thread_id);
                        return;
                    }
                }
            }
        };

        // Wait out any run started elsewhere (e.g. the web UI) so `send` never hits AlreadyRunning.
        wait_until_idle(&state, thread_id).await;

        // Subscribe before sending: same worker processes both in order, so no events are missed.
        let events = match state.runner.subscribe(thread_id, None).await {
            Ok(subscription) => subscription.events,
            Err(error) => {
                tracing::warn!(%thread_id, %error, "failed to subscribe Telegram session");
                continue;
            }
        };
        let run_id = match state
            .runner
            .send(
                thread_id,
                AgentRequest {
                    content: message.text,
                },
            )
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => {
                tracing::warn!(%thread_id, %error, "failed to start Telegram agent run");
                send_telegram_message(
                    &state,
                    message.chat_id,
                    message.topic_id,
                    "Friday could not start: please try again.",
                )
                .await;
                continue;
            }
        };

        forward_run(
            state.clone(),
            events,
            run_id.0,
            user_id,
            thread_id,
            message.chat_id,
            message.topic_id,
        )
        .await;
    }
}

async fn wait_until_idle(state: &ServerState, thread_id: Uuid) {
    for _ in 0..600 {
        match state.runner.status(thread_id).await {
            Ok(ThreadStatus::Idle) | Err(_) => return,
            Ok(ThreadStatus::Running { .. }) => sleep(Duration::from_millis(200)).await,
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
        let mut ix = state.telegram_sessions.interactions.lock().unwrap();
        ix.callbacks.insert(
            approve.clone(),
            CallbackAction::Approval {
                thread_id,
                approval_id,
                approved: true,
                sibling: deny.clone(),
            },
        );
        ix.callbacks.insert(
            deny.clone(),
            CallbackAction::Approval {
                thread_id,
                approval_id,
                approved: false,
                sibling: approve.clone(),
            },
        );
    }

    let keyboard = vec![vec![
        InlineButton {
            text: "✅ Approve".to_string(),
            callback_data: approve,
        },
        InlineButton {
            text: "❌ Deny".to_string(),
            callback_data: deny,
        },
    ]];
    send_telegram_buttons(state, chat_id, topic_id, &format!("⚠️ {message}"), keyboard).await;
}

async fn present_quiz(
    state: &ServerState,
    thread_id: Uuid,
    chat_id: i64,
    topic_id: Option<i64>,
    quiz_id: Uuid,
    questions: Vec<QuizQuestion>,
) {
    if questions.is_empty() {
        let _ = state
            .runner
            .answer_quiz(thread_id, quiz_id, Vec::new())
            .await;
        return;
    }

    {
        let mut ix = state.telegram_sessions.interactions.lock().unwrap();
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
        let mut ix = state.telegram_sessions.interactions.lock().unwrap();
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

/// Records an answer for `question_index`, then either advances to the next question or submits the
/// completed quiz to the agent.
async fn answer_quiz_question(
    state: &ServerState,
    quiz_id: Uuid,
    question_index: usize,
    answer: String,
) {
    let submit = {
        let mut ix = state.telegram_sessions.interactions.lock().unwrap();
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
        if done {
            ix.quizzes.remove(&quiz_id);
            ix.awaiting_text.remove(&(chat_id, topic_id));
            answers.map(|answers| (thread_id, answers))
        } else {
            None
        }
    };

    match submit {
        Some((thread_id, answers)) => {
            let _ = state.runner.answer_quiz(thread_id, quiz_id, answers).await;
        }
        None => send_quiz_question(state, quiz_id).await,
    }
}

fn pending_free_form_quiz(
    state: &ServerState,
    chat_id: i64,
    topic_id: Option<i64>,
) -> Option<(Uuid, usize)> {
    let ix = state.telegram_sessions.interactions.lock().unwrap();
    let quiz_id = *ix.awaiting_text.get(&(chat_id, topic_id))?;
    let quiz = ix.quizzes.get(&quiz_id)?;
    Some((quiz_id, quiz.current))
}

async fn handle_callback(state: &ServerState, callback: CallbackQuery) {
    let Some(token) = callback.data.clone() else {
        return;
    };
    let action = state
        .telegram_sessions
        .interactions
        .lock()
        .unwrap()
        .callbacks
        .remove(&token);
    let Some(action) = action else {
        answer_callback_query(state, &callback.id, "This action is no longer available.").await;
        return;
    };

    // Only the thread owner may resolve a prompt — buttons can be visible to a whole group.
    let owner = thread_owner(state, action.thread_id()).await;
    let caller = user_for_telegram_id(state, callback.from.id)
        .await
        .ok()
        .flatten();
    if owner.is_none() || owner != caller {
        answer_callback_query(state, &callback.id, "Not allowed.").await;
        return;
    }

    match action {
        CallbackAction::Approval {
            thread_id,
            approval_id,
            approved,
            sibling,
        } => {
            state
                .telegram_sessions
                .interactions
                .lock()
                .unwrap()
                .callbacks
                .remove(&sibling);
            let _ = state
                .runner
                .resolve_approval(thread_id, approval_id, approved)
                .await;
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
            answer_quiz_question(state, quiz_id, question_index, answer).await;
        }
    }
}

fn clear_interactions(state: &ServerState, thread_id: Uuid, chat_id: i64, topic_id: Option<i64>) {
    let mut ix = state.telegram_sessions.interactions.lock().unwrap();
    ix.callbacks
        .retain(|_, action| action.thread_id() != thread_id);
    ix.quizzes.retain(|_, quiz| quiz.thread_id != thread_id);
    ix.awaiting_text.remove(&(chat_id, topic_id));
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
    message_thread_id: Option<i64>,
    text: &str,
    keyboard: Vec<Vec<InlineButton>>,
) -> Option<TelegramSentMessage> {
    let text: String = text.chars().take(4096).collect();
    let request = SendButtonsRequest {
        chat_id,
        message_thread_id,
        text: &text,
        reply_markup: InlineKeyboardMarkup {
            inline_keyboard: keyboard,
        },
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

async fn create_telegram_thread(
    state: &ServerState,
    user_id: Uuid,
) -> Result<Uuid, TelegramApiError> {
    let thread_id = Uuid::now_v7();
    threads::insert()
        .id(thread_id)
        .owner(user_id)
        .title(DEFAULT_THREAD_TITLE)
        .execute(&state.db)
        .await
        .map_err(|_| TelegramApiError::Internal)?;

    Ok(thread_id)
}

/// Returns the thread and whether it was just created.
async fn ensure_telegram_thread(
    state: &ServerState,
    user_id: Uuid,
    message: &TelegramMessage,
) -> Result<(Uuid, bool), TelegramApiError> {
    let topic_id = message.topic_id().unwrap_or(0);
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

async fn forward_run(
    state: Arc<ServerState>,
    mut events: tokio::sync::broadcast::Receiver<crate::runner::AgentEvent>,
    run_id: Uuid,
    user_id: Uuid,
    thread_id: Uuid,
    chat_id: i64,
    message_thread_id: Option<i64>,
) {
    let mut content = String::new();
    let draft_id = telegram_draft_id(run_id);
    let mut last_draft_text = String::new();
    let mut finalized = false;
    let mut draft_tick = interval(Duration::from_millis(700));

    loop {
        tokio::select! {
            event = events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                };
                if event.run_id.map(|id| id.0) != Some(run_id) {
                    continue;
                }
                match event.kind {
                    AgentEventKind::AgentDelta { content: delta } => {
                        content.push_str(&delta);
                    }
                    AgentEventKind::AgentMessageCommitted { message_id, .. } => {
                        if let Some(final_content) = agent_message_content(&state, message_id).await {
                            content = final_content;
                            finalize_telegram_stream(
                                &state,
                                user_id,
                                thread_id,
                                chat_id,
                                message_thread_id,
                                &content,
                            )
                            .await;
                            finalized = true;
                        }
                    }
                    AgentEventKind::RunFailed { error } => {
                        send_telegram_message(
                            &state,
                            chat_id,
                            message_thread_id,
                            &format!("Friday failed: {error}"),
                        )
                        .await;
                        break;
                    }
                    AgentEventKind::RunFinished => {
                        if !finalized {
                            finalize_latest_telegram_response(
                                &state,
                                user_id,
                                thread_id,
                                chat_id,
                                message_thread_id,
                                &mut content,
                            )
                            .await;
                        }
                        break;
                    }
                    AgentEventKind::RunCancelled => break,
                    AgentEventKind::WaitingForApproval { approval_id, message } => {
                        present_approval(
                            &state,
                            thread_id,
                            chat_id,
                            message_thread_id,
                            approval_id,
                            &message,
                        )
                        .await;
                    }
                    AgentEventKind::WaitingForQuiz { quiz_id, questions } => {
                        present_quiz(
                            &state,
                            thread_id,
                            chat_id,
                            message_thread_id,
                            quiz_id,
                            questions,
                        )
                        .await;
                    }
                    _ => {}
                }
            }
            _ = draft_tick.tick() => {
                let text = content.trim();
                if !text.is_empty() && text != last_draft_text {
                    last_draft_text = text.to_string();
                    let state = state.clone();
                    let text = last_draft_text.clone();
                    tokio::spawn(async move {
                        let _ = send_telegram_rich_message_draft(
                            &state,
                            chat_id,
                            message_thread_id,
                            draft_id,
                            &text,
                        )
                        .await;
                    });
                }
            }
        };
    }

    // Drop any prompts left over if the run ended (finished/failed/cancelled) before resolution.
    clear_interactions(&state, thread_id, chat_id, message_thread_id);
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
    message_thread_id: Option<i64>,
    draft_id: i64,
    text: &str,
) -> bool {
    let Some(token) = bot_token(state) else {
        return false;
    };

    let markdown = rich_markdown(text);
    let request = SendRichMessageDraftRequest {
        chat_id,
        message_thread_id,
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
    message_thread_id: Option<i64>,
    text: &str,
) -> RichSend {
    let Some(token) = bot_token(state) else {
        return RichSend::Failed;
    };

    let markdown = rich_markdown(text);
    let request = SendRichMessageRequest {
        chat_id,
        message_thread_id,
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
    message_thread_id: Option<i64>,
    text: &str,
) -> Option<TelegramSentMessage> {
    let token = bot_token(state)?;

    let text: String = text.chars().take(4096).collect();
    let request = SendMessageRequest {
        chat_id,
        message_thread_id,
        text: &text,
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
        .and_then(|t| t.webhook_secret.as_deref());

    let Some(expected) = expected.filter(|s| !s.is_empty()) else {
        return Ok(());
    };

    let actual = headers
        .get(TELEGRAM_SECRET_HEADER)
        .and_then(|v| v.to_str().ok());

    if actual == Some(expected) {
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

fn connect_code(text: Option<&str>) -> Option<&str> {
    let text = text?.trim();
    let mut parts = text.split_whitespace();
    let command = parts.next()?;
    let command = command.split('@').next()?;
    if !command.eq_ignore_ascii_case("/connect") && !command.eq_ignore_ascii_case("/start") {
        return None;
    }
    parts.next().filter(|code| !code.is_empty())
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

fn generate_code() -> String {
    format!("{:06}", OsRng.next_u32() % 1_000_000)
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
    text.chars().take(4096).collect()
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
    message_thread_id: Option<i64>,
    direct_messages_topic: Option<DirectMessagesTopic>,
    reply_to_message: Option<TelegramReplyMessage>,
    forum_topic_created: Option<ForumTopicCreated>,
}

impl TelegramMessage {
    fn topic_id(&self) -> Option<i64> {
        self.message_thread_id.or_else(|| {
            self.direct_messages_topic
                .as_ref()
                .map(|topic| topic.topic_id)
        })
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
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForumTopicCreated {}

#[derive(Serialize)]
struct SendMessageRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    text: &'a str,
}

#[derive(Serialize)]
struct SendRichMessageDraftRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    draft_id: i64,
    rich_message: InputRichMessage<'a>,
}

#[derive(Serialize)]
struct SendRichMessageRequest<'a> {
    chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
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
    text: &'a str,
    reply_markup: InlineKeyboardMarkup,
}

#[derive(Serialize)]
struct InlineKeyboardMarkup {
    inline_keyboard: Vec<Vec<InlineButton>>,
}

#[derive(Serialize)]
struct InlineButton {
    text: String,
    callback_data: String,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parses_connect_command() {
        assert_eq!(connect_code(Some("/connect 123456")), Some("123456"));
        assert_eq!(connect_code(Some("/start 123456")), Some("123456"));
        assert_eq!(
            connect_code(Some("/connect@friday_bot 123456")),
            Some("123456")
        );
        assert_eq!(connect_code(Some("/connect")), None);
    }

    use async_trait::async_trait;
    use minisql::ConnectionPool;
    use tokio::sync::broadcast;

    use crate::{
        config::Config,
        db::users,
        runner::{
            AgentEvent, AgentPool, AgentPoolError, RunId, ThreadSnapshot, ThreadSubscription,
        },
    };

    #[derive(Default)]
    struct FakePool {
        senders: Mutex<HashMap<Uuid, broadcast::Sender<AgentEvent>>>,
        received: Mutex<Vec<String>>,
        approvals: Mutex<Vec<(Uuid, Uuid, bool)>>,
        quiz_answers: Mutex<Vec<(Uuid, Uuid, Vec<String>)>>,
    }

    impl FakePool {
        fn sender(&self, thread_id: Uuid) -> broadcast::Sender<AgentEvent> {
            self.senders
                .lock()
                .unwrap()
                .entry(thread_id)
                .or_insert_with(|| broadcast::channel(16).0)
                .clone()
        }
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
            let tx = self.sender(thread_id);
            let _ = tx.send(AgentEvent {
                seq: 1,
                thread_id,
                run_id: Some(run_id),
                kind: AgentEventKind::RunStarted,
            });
            let _ = tx.send(AgentEvent {
                seq: 2,
                thread_id,
                run_id: Some(run_id),
                kind: AgentEventKind::RunFinished,
            });
            Ok(run_id)
        }

        async fn subscribe(
            &self,
            thread_id: Uuid,
            _after: Option<u64>,
        ) -> Result<ThreadSubscription, AgentPoolError> {
            Ok(ThreadSubscription {
                snapshot: ThreadSnapshot {
                    thread_id,
                    last_event_seq: 0,
                    status: ThreadStatus::Idle,
                    in_progress: None,
                    pending_approval: None,
                    pending_quiz: None,
                },
                events: self.sender(thread_id).subscribe(),
                replay: Vec::new(),
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
            model_config: Arc::new(friday_agent::AgentConfig {
                model_registry: friday_agent::ModelRegistry::default(),
                max_iterations: 1,
            }),
            vfs: None,
            telegram_sessions: Arc::new(TelegramSessions::default()),
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

    fn callback(token: String, from_id: i64, chat_id: i64, text: &str) -> CallbackQuery {
        CallbackQuery {
            id: "cb".to_string(),
            from: TelegramUser {
                id: from_id,
                username: None,
                first_name: None,
                last_name: None,
            },
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

    fn find_token(state: &ServerState, predicate: impl Fn(&CallbackAction) -> bool) -> String {
        state
            .telegram_sessions
            .interactions
            .lock()
            .unwrap()
            .callbacks
            .iter()
            .find_map(|(token, action)| predicate(action).then(|| token.clone()))
            .expect("token not found")
    }

    #[tokio::test]
    async fn session_queues_concurrent_messages_in_order() {
        let pool = Arc::new(FakePool::default());
        let state = build_state(pool.clone()).await;

        let thread_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        // Enqueue three messages back-to-back for the same thread, as concurrent webhooks would.
        for i in 0..3 {
            state.telegram_sessions.clone().dispatch(
                state.clone(),
                thread_id,
                user_id,
                QueuedMessage {
                    text: format!("msg{i}"),
                    chat_id: 1,
                    topic_id: None,
                },
            );
        }

        for _ in 0..200 {
            if pool.received.lock().unwrap().len() == 3 {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }

        let received = pool.received.lock().unwrap().clone();
        assert_eq!(received, vec!["msg0", "msg1", "msg2"]);
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
                .telegram_sessions
                .interactions
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
}
