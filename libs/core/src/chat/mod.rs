use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_stream::stream;
use futures::{Stream, StreamExt, future::BoxFuture};
use llm::{API, Completion, CompletionChoice, CompletionRequest, Message, OpenAI, Role, UnnamedToolChoice};
use minisql::{ConnectionPool, Migration, migrations};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

use crate::tools::{Tool, ToolArg};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, uniffi::Enum)]
pub enum TurnRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolInvocationStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl ToolInvocationStatus {
    fn as_str(self) -> &'static str {
        match self {
            ToolInvocationStatus::Queued => "queued",
            ToolInvocationStatus::Running => "running",
            ToolInvocationStatus::Completed => "completed",
            ToolInvocationStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: Uuid,
    pub name: String,
    pub arguments_json: String,
    pub result_json: Option<String>,
    pub status: ToolInvocationStatus,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub preview_text: String,
    pub is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct ChatMessage {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub user_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub provider_id: String,
    pub model_id: String,
    pub model_name: String,
    pub role: TurnRole,
    pub thinking: Option<String>,
    pub content: String,
    pub tool_call: Option<String>,
    pub tool_result: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_done: bool,
    pub usage: Option<String>,
}

impl ChatMessage {
    pub fn new(role: TurnRole, content: impl Into<String>) -> Self {
        let now = now_millis();
        Self {
            id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            user_id: None,
            parent_id: None,
            provider_id: String::new(),
            model_id: String::new(),
            model_name: String::new(),
            role,
            thinking: None,
            content: content.into(),
            tool_call: None,
            tool_result: None,
            created_at: now,
            updated_at: now,
            is_done: false,
            usage: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct LangModel {
    pub provider: String,
    pub model: String,
    pub provider_name: String,
    pub model_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, uniffi::Enum)]
pub enum ChatProviderKind {
    OpenAICompatible,
    Ollama,
    Anthropic,
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct ChatProviderConfiguration {
    pub id: Uuid,
    pub name: String,
    pub kind: ChatProviderKind,
    pub base_url: String,
    pub token: String,
    pub default_model: String,
}

pub trait ChatTransport: Send + Sync {
    fn provider_id(&self) -> &str;
    fn list_models<'a>(&'a self) -> BoxFuture<'a, Vec<LangModel>>;
    fn get_response<'a>(
        &'a self,
        model_id: &'a str,
        messages: &'a [ChatMessage],
        tools: &'a [Arc<dyn Tool>],
    ) -> BoxFuture<'a, Result<Completion, llm::Error>>;
    fn stream_response<'a>(
        &'a self,
        model_id: &'a str,
        messages: &'a [ChatMessage],
        tools: &'a [Arc<dyn Tool>],
    ) -> Pin<Box<dyn Stream<Item = Result<ChatMessage, llm::Error>> + Send + 'static>>;
}

#[derive(Debug, Clone)]
pub struct DirectChatTransport {
    provider_id: String,
    api: API,
    token: String,
}

impl DirectChatTransport {
    pub fn new(provider_id: impl Into<String>, api: API, token: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            api,
            token: token.into(),
        }
    }

    pub fn from_provider(provider: ChatProviderConfiguration) -> Self {
        let provider_id = provider.id.to_string();
        let token = provider.token.clone();
        let api = match provider.kind {
            ChatProviderKind::OpenAICompatible => OpenAI::new(&provider.base_url),
            ChatProviderKind::Ollama => llm::Ollama::new(&provider.base_url),
            ChatProviderKind::Anthropic => llm::Anthropic::new(&provider.base_url),
            ChatProviderKind::Mock => llm::Mock::new().into(),
        };
        Self {
            provider_id,
            api,
            token,
        }
    }

    fn completion_request(
        &self,
        model_id: &str,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> CompletionRequest {
        let mut request = CompletionRequest::new(
            model_id,
            &messages
                .iter()
                .map(|m| Message {
                    role: map_role(m.role),
                    content: m.content.clone(),
                    thinking: m.thinking.clone(),
                    tool_call_id: None,
                })
                .collect::<Vec<_>>(),
        );

        if !tools.is_empty() {
            request = request
                .tools(tools.iter().map(|tool| tool.as_llm()).collect())
                .tool_choice(UnnamedToolChoice::Auto);
        }
        request
    }
}

impl ChatTransport for DirectChatTransport {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn list_models<'a>(&'a self) -> BoxFuture<'a, Vec<LangModel>> {
        Box::pin(async move {
            match self.api.list_models(&self.token).await {
                Ok(models) => {
                    let mut mapped = models
                        .into_iter()
                        .map(|model| LangModel {
                            provider: self.provider_id.clone(),
                            model: model.id.clone(),
                            provider_name: self.provider_id.clone(),
                            model_name: model.id,
                        })
                        .collect::<Vec<_>>();
                    mapped.sort_by(|a, b| a.model.cmp(&b.model));
                    mapped
                }
                Err(_) => vec![],
            }
        })
    }

    fn get_response<'a>(
        &'a self,
        model_id: &'a str,
        messages: &'a [ChatMessage],
        tools: &'a [Arc<dyn Tool>],
    ) -> BoxFuture<'a, Result<Completion, llm::Error>> {
        Box::pin(async move {
            let request = self.completion_request(model_id, messages, tools);
            self.api.get_completion(&self.token, request).await
        })
    }

    fn stream_response<'a>(
        &'a self,
        model_id: &'a str,
        messages: &'a [ChatMessage],
        tools: &'a [Arc<dyn Tool>],
    ) -> Pin<Box<dyn Stream<Item = Result<ChatMessage, llm::Error>> + Send + 'static>> {
        let request = self.completion_request(model_id, messages, tools);
        let provider_id = self.provider_id.clone();
        let model_id = model_id.to_owned();
        let thread_id = messages.last().map(|m| m.thread_id).unwrap_or_else(Uuid::new_v4);
        let parent_id = messages.last().map(|m| m.id);
        let stream = self.api.stream_completion(&self.token, request);

        Box::pin(stream! {
            let mut tool_calls: Vec<ModelFunctionCall> = Vec::new();
            let now = now_millis();
            let mut response = ChatMessage {
                id: Uuid::new_v4(),
                thread_id,
                user_id: None,
                parent_id,
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
                model_name: model_id.clone(),
                role: TurnRole::Assistant,
                thinking: None,
                content: String::new(),
                tool_call: None,
                tool_result: None,
                created_at: now,
                updated_at: now,
                is_done: false,
                usage: None,
            };

            futures::pin_mut!(stream);
            while let Some(item) = stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        response.is_done = true;
                        response.updated_at = now_millis();
                        yield Ok(response.clone());
                        yield Err(error);
                        return;
                    }
                };

                for choice in chunk.choices {
                    merge_tool_calls(&mut tool_calls, &choice);
                    if !tool_calls.is_empty() {
                        response.tool_call = json_string(&tool_calls);
                        response.updated_at = now_millis();
                        yield Ok(response.clone());
                    }

                    let message_content = choice.message.as_ref().map(|m| m.content.clone());
                    let message_thinking =
                        choice.message.as_ref().and_then(|m| m.thinking.clone());

                    let token = choice
                        .delta
                        .as_ref()
                        .and_then(|d| d.content.clone())
                        .or(choice.text)
                        .or(message_content)
                        .unwrap_or_default();
                    if !token.is_empty() {
                        response.content.push_str(&token);
                        response.updated_at = now_millis();
                        yield Ok(response.clone());
                    }

                    let reasoning = choice
                        .delta
                        .as_ref()
                        .and_then(|d| d.thinking.clone())
                        .or(message_thinking)
                        .unwrap_or_default();
                    if !reasoning.is_empty() {
                        match response.thinking.as_mut() {
                            Some(thinking) => thinking.push_str(&reasoning),
                            None => response.thinking = Some(reasoning),
                        }
                        response.updated_at = now_millis();
                        yield Ok(response.clone());
                    }
                }
            }

            response.is_done = true;
            response.updated_at = now_millis();
            yield Ok(response);
        })
    }
}

#[derive(Debug, Error)]
pub enum ChatStreamError {
    #[error("provider not selected")]
    ProviderNotSelected,
    #[error("model not selected")]
    ModelNotSelected,
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("max tool iterations exceeded")]
    MaxToolIterationsExceeded,
    #[error("transport error: {0}")]
    Transport(String),
}

pub trait ChatStorage: Send + Sync {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>>;
    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()>;
}

#[derive(Debug, Default)]
pub struct NullChatStorage;

impl ChatStorage for NullChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async { vec![] })
    }

    fn append_message<'a>(&'a self, _message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

#[derive(Debug, Default)]
pub struct MockChatStorage {
    messages: Mutex<Vec<ChatMessage>>,
}

impl MockChatStorage {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        let mut sorted = messages;
        sorted.sort_by_key(|m| m.created_at);
        Self {
            messages: Mutex::new(sorted),
        }
    }
}

impl ChatStorage for MockChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move { self.messages.lock().await.clone() })
    }

    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.messages.lock().await.push(message);
        })
    }
}

pub struct LocalChatStorage {
    chat_thread_id: Uuid,
    database: ConnectionPool,
    migrations_ready: OnceCell<()>,
}

impl LocalChatStorage {
    pub fn new(thread_id: Uuid, database: ConnectionPool) -> Self {
        Self {
            chat_thread_id: thread_id,
            database,
            migrations_ready: OnceCell::new(),
        }
    }

    async fn ensure_migrations(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.migrations_ready
            .get_or_try_init(|| async {
                let migrations: Vec<Migration> = chat_schema::get_migrations();
                self.database.initialize_database(migrations).await
            })
            .await
            .map(|_| ())
    }
}

impl ChatStorage for LocalChatStorage {
    fn list_messages<'a>(&'a self) -> BoxFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move {
            if self.ensure_migrations().await.is_err() {
                return vec![];
            }

            let result = chat_schema::chat_messages::select()
                .where_(chat_schema::chat_messages::thread_id.eq(self.chat_thread_id))
                .order_by_asc(chat_schema::chat_messages::created_at)
                .all(&self.database)
                .await;

            match result {
                Ok(rows) => rows.into_iter().map(StoredChatMessage::from_row).collect(),
                Err(_) => vec![],
            }
        })
    }

    fn append_message<'a>(&'a self, message: ChatMessage) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if self.ensure_migrations().await.is_err() {
                return;
            }

            let now = now_millis();
            let preview = message.content.trim().to_owned();
            let existing = chat_schema::chat_threads::select()
                .where_(chat_schema::chat_threads::id.eq(self.chat_thread_id))
                .limit(1)
                .all(&self.database)
                .await;

            if let Ok(mut existing) = existing {
                if let Some(thread) = existing.pop() {
                    let updated_at = thread.updated_at.max(message.updated_at);
                    let title = if thread.title.is_empty() {
                        if preview.is_empty() {
                            "Chat".to_owned()
                        } else {
                            preview.chars().take(80).collect::<String>()
                        }
                    } else {
                        thread.title
                    };
                    let next_preview = if preview.is_empty() {
                        thread.preview_text
                    } else {
                        preview.clone()
                    };
                    let _ = self
                        .database
                        .query_with_params(
                            "UPDATE chat_threads SET title = ?, updated_at = ?, preview_text = ? WHERE id = ?",
                            vec![
                                minisql::Value::Text(title),
                                minisql::Value::Integer(updated_at),
                                minisql::Value::Text(next_preview),
                                minisql::Value::Uuid(self.chat_thread_id),
                            ],
                        )
                        .await;
                } else {
                    let _ = chat_schema::chat_threads::insert()
                        .id(self.chat_thread_id)
                        .user_id(message.user_id)
                        .title(if preview.is_empty() {
                            "Chat".to_owned()
                        } else {
                            preview.chars().take(80).collect::<String>()
                        })
                        .created_at(message.created_at)
                        .updated_at(message.updated_at)
                        .preview_text(preview.clone())
                        .is_pinned(false)
                        .execute(&self.database)
                        .await;
                }
            }

            let stored = StoredChatMessage::from_domain(self.chat_thread_id, message, now);
            let _ = chat_schema::chat_messages::insert()
                .id(stored.id)
                .thread_id(stored.thread_id)
                .user_id(stored.user_id)
                .parent_id(stored.parent_id)
                .provider_id(stored.provider_id)
                .model_id(stored.model_id)
                .model_name(stored.model_name)
                .role(stored.role)
                .thinking(stored.thinking)
                .content(stored.content)
                .tool_call(stored.tool_call)
                .tool_result(stored.tool_result)
                .created_at(stored.created_at)
                .updated_at(stored.updated_at)
                .is_done(stored.is_done)
                .usage(stored.usage)
                .execute(&self.database)
                .await;
        })
    }
}

#[derive(Clone, uniffi::Object)]
pub struct ChatService {
    transports: Vec<Arc<dyn ChatTransport>>,
    storage: Arc<dyn ChatStorage>,
    state: Arc<Mutex<ChatServiceState>>,
}

#[derive(Debug, Default)]
struct ChatServiceState {
    provider_id: Option<String>,
    model_id: Option<String>,
    has_loaded_storage: bool,
    messages: Vec<ChatMessage>,
}

impl ChatService {
    pub fn new(transports: Vec<Arc<dyn ChatTransport>>, storage: Arc<dyn ChatStorage>) -> Self {
        let mut transports = transports;
        transports.sort_by(|a, b| a.provider_id().cmp(b.provider_id()));
        Self {
            transports,
            storage,
            state: Arc::new(Mutex::new(ChatServiceState::default())),
        }
    }

    pub async fn list_models(&self) -> Vec<LangModel> {
        let mut merged = Vec::new();
        for transport in &self.transports {
            merged.extend(transport.list_models().await);
        }
        merged
    }

    pub async fn get_messages(&self) -> Vec<ChatMessage> {
        self.ensure_messages_loaded().await;
        self.state.lock().await.messages.clone()
    }

    pub async fn set_model(&self, provider_id: String, model_id: String) {
        let mut state = self.state.lock().await;
        state.provider_id = Some(provider_id);
        state.model_id = Some(model_id);
    }

    pub async fn add_message(
        &self,
        tools: Vec<Arc<dyn Tool>>,
        next: ChatMessage,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatMessage, ChatStreamError>> + Send + 'static>> {
        self.ensure_messages_loaded().await;
        self.append_message(next.clone()).await;

        let (provider_id, model_id, messages) = {
            let state = self.state.lock().await;
            (
                state.provider_id.clone(),
                state.model_id.clone(),
                state.messages.clone(),
            )
        };

        let Some(provider_id) = provider_id else {
            return Box::pin(stream! {
                yield Err(ChatStreamError::ProviderNotSelected);
            });
        };
        let Some(model_id) = model_id else {
            return Box::pin(stream! {
                yield Err(ChatStreamError::ModelNotSelected);
            });
        };
        let Some(transport) = self
            .transports
            .iter()
            .find(|transport| transport.provider_id() == provider_id)
            .cloned()
        else {
            return Box::pin(stream! {
                yield Err(ChatStreamError::UnknownProvider(provider_id));
            });
        };

        let this = self.clone();
        if tools.is_empty() {
            let upstream = transport.stream_response(&model_id, &messages, &tools);
            return Box::pin(stream! {
                let mut latest: Option<ChatMessage> = None;
                futures::pin_mut!(upstream);
                while let Some(item) = upstream.next().await {
                    match item {
                        Ok(partial) => {
                            latest = Some(partial.clone());
                            yield Ok(partial);
                        }
                        Err(error) => {
                            if let Some(latest) = latest {
                                this.append_message(latest).await;
                            }
                            yield Err(ChatStreamError::Transport(error.to_string()));
                            return;
                        }
                    }
                }

                if let Some(latest) = latest {
                    this.append_message(latest).await;
                }
            });
        }

        Box::pin(stream! {
            let mut working_messages = messages;
            let thread_id = working_messages
                .last()
                .map(|m| m.thread_id)
                .unwrap_or(next.thread_id);

            for _ in 0..8 {
                let upstream = transport.stream_response(&model_id, &working_messages, &tools);
                let mut latest: Option<ChatMessage> = None;

                futures::pin_mut!(upstream);
                while let Some(item) = upstream.next().await {
                    match item {
                        Ok(partial) => {
                            latest = Some(partial.clone());
                            yield Ok(partial);
                        }
                        Err(error) => {
                            if let Some(mut latest) = latest {
                                latest.is_done = true;
                                latest.updated_at = now_millis();
                                this.append_message(latest).await;
                            }
                            yield Err(ChatStreamError::Transport(error.to_string()));
                            return;
                        }
                    }
                }

                let Some(mut assistant) = latest else {
                    return;
                };
                assistant.is_done = true;
                assistant.updated_at = now_millis();
                this.append_message(assistant.clone()).await;
                working_messages.push(assistant.clone());

                let calls = extract_function_calls(assistant.tool_call.as_deref());
                if calls.is_empty() {
                    return;
                }

                let mut tool_result_entries = Vec::new();
                for call in calls {
                    let started_at = now_millis();
                    let mut invocation = ToolInvocation {
                        id: Uuid::new_v4(),
                        name: call.name.clone(),
                        arguments_json: call.arguments.clone(),
                        result_json: None,
                        status: ToolInvocationStatus::Running,
                        started_at,
                        ended_at: None,
                    };

                    let result = if let Some(tool) = tools.iter().find(|tool| tool.id() == call.name) {
                        let parsed_args = parse_tool_args(&call.arguments);
                        tool.execute(&parsed_args).await
                    } else {
                        format!("Error: Unknown tool '{}'.", call.name)
                    };

                    invocation.status = if result.starts_with("Error:") {
                        ToolInvocationStatus::Failed
                    } else {
                        ToolInvocationStatus::Completed
                    };
                    invocation.result_json = Some(result.clone());
                    invocation.ended_at = Some(now_millis());
                    tool_result_entries.push(tool_result_dictionary(&call, &invocation));

                    let tool_message = ChatMessage {
                        id: Uuid::new_v4(),
                        thread_id,
                        user_id: None,
                        parent_id: Some(assistant.id),
                        provider_id: provider_id.clone(),
                        model_id: model_id.clone(),
                        model_name: model_id.clone(),
                        role: TurnRole::Tool,
                        thinking: None,
                        content: result,
                        tool_call: None,
                        tool_result: assistant.tool_result.clone(),
                        created_at: now_millis(),
                        updated_at: now_millis(),
                        is_done: true,
                        usage: None,
                    };
                    this.append_message(tool_message.clone()).await;
                    working_messages.push(tool_message);
                }

                assistant.tool_result = json_string(&tool_result_entries);
                assistant.updated_at = now_millis();
            }

            yield Err(ChatStreamError::MaxToolIterationsExceeded);
        })
    }

    async fn ensure_messages_loaded(&self) {
        let should_load = {
            let mut state = self.state.lock().await;
            if state.has_loaded_storage {
                false
            } else {
                state.has_loaded_storage = true;
                true
            }
        };

        if !should_load {
            return;
        }

        let mut loaded = self.storage.list_messages().await;
        loaded.sort_by_key(|message| message.created_at);
        self.state.lock().await.messages = loaded;
    }

    async fn append_message(&self, message: ChatMessage) {
        self.state.lock().await.messages.push(message.clone());
        self.storage.append_message(message).await;
    }
}

#[derive(Debug, Error, uniffi::Error)]
pub enum ChatFFIError {
    #[error("chat stream failed: {0}")]
    Stream(String),
    #[error("no response produced")]
    EmptyResponse,
}

#[uniffi::export]
impl ChatService {
    #[uniffi::constructor]
    pub fn new_with_providers(providers: Vec<ChatProviderConfiguration>) -> Arc<Self> {
        let transports: Vec<Arc<dyn ChatTransport>> = providers
            .into_iter()
            .map(DirectChatTransport::from_provider)
            .map(|transport| Arc::new(transport) as Arc<dyn ChatTransport>)
            .collect();
        let storage: Arc<dyn ChatStorage> = Arc::new(NullChatStorage);
        Arc::new(Self::new(transports, storage))
    }

    #[uniffi::constructor]
    pub fn new_ollama(base_url: String, token: String) -> Arc<Self> {
        Self::new_with_providers(vec![ChatProviderConfiguration {
            id: Uuid::new_v4(),
            name: "Local Ollama".to_owned(),
            kind: ChatProviderKind::Ollama,
            base_url,
            token,
            default_model: String::new(),
        }])
    }

    pub async fn ffi_list_models(&self) -> Vec<LangModel> {
        self.list_models().await
    }

    pub async fn ffi_get_messages(&self) -> Vec<ChatMessage> {
        self.get_messages().await
    }

    pub async fn ffi_set_model(&self, provider_id: String, model_id: String) {
        self.set_model(provider_id, model_id).await;
    }

    pub async fn add_message_collect(
        &self,
        tools_enabled: bool,
        next: ChatMessage,
    ) -> Result<Vec<ChatMessage>, ChatFFIError> {
        let tools: Vec<Arc<dyn Tool>> = if tools_enabled {
            vec![Arc::new(crate::tools::JSTool::new()) as Arc<dyn Tool>]
        } else {
            vec![]
        };

        let mut stream = self.add_message(tools, next).await;
        let mut chunks = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(message) => chunks.push(message),
                Err(error) => return Err(ChatFFIError::Stream(error.to_string())),
            }
        }
        Ok(chunks)
    }

    pub async fn add_message_final(
        &self,
        tools_enabled: bool,
        next: ChatMessage,
    ) -> Result<ChatMessage, ChatFFIError> {
        let mut chunks = self.add_message_collect(tools_enabled, next).await?;
        chunks.pop().ok_or(ChatFFIError::EmptyResponse)
    }
}

fn map_role(role: TurnRole) -> Role {
    match role {
        TurnRole::System => Role::System,
        TurnRole::User => Role::User,
        TurnRole::Assistant => Role::Assistant,
        TurnRole::Tool => Role::Tool,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelFunctionCall {
    name: String,
    arguments: String,
    #[serde(rename = "callID", skip_serializing_if = "Option::is_none")]
    call_id: Option<String>,
}

fn merge_tool_calls(target: &mut Vec<ModelFunctionCall>, choice: &CompletionChoice) {
    let Some(delta) = &choice.delta else {
        return;
    };
    let Some(tool_calls) = &delta.tool_calls else {
        return;
    };

    for incoming in tool_calls {
        let index = incoming.index.unwrap_or(target.len());
        while target.len() <= index {
            target.push(ModelFunctionCall {
                name: String::new(),
                arguments: String::new(),
                call_id: None,
            });
        }

        if let Some(id) = incoming.id.clone() {
            target[index].call_id = Some(id);
        }
        if let Some(function) = &incoming.function {
            if let Some(name) = function.name.clone() {
                target[index].name = name;
            }
            if let Some(arguments) = function.arguments.clone() {
                target[index].arguments.push_str(&arguments);
            }
        }
    }

    target.retain(|call| !call.name.is_empty());
}

fn extract_function_calls(raw: Option<&str>) -> Vec<ModelFunctionCall> {
    raw.and_then(|raw| serde_json::from_str::<Vec<ModelFunctionCall>>(raw).ok())
        .unwrap_or_default()
}

fn parse_tool_args(arguments_json: &str) -> Vec<ToolArg> {
    let parsed = serde_json::from_str::<HashMap<String, serde_json::Value>>(arguments_json);
    let Ok(parsed) = parsed else {
        return vec![];
    };

    parsed
        .into_iter()
        .map(|(name, value)| {
            let value = match value {
                serde_json::Value::String(value) => value,
                _ => value.to_string(),
            };
            ToolArg { name, value }
        })
        .collect()
}

fn tool_result_dictionary(
    call: &ModelFunctionCall,
    invocation: &ToolInvocation,
) -> HashMap<String, String> {
    let mut out = HashMap::from([
        ("name".to_owned(), call.name.clone()),
        ("status".to_owned(), invocation.status.as_str().to_owned()),
        (
            "result".to_owned(),
            invocation.result_json.clone().unwrap_or_default(),
        ),
    ]);
    if let Some(call_id) = &call.call_id {
        out.insert("callID".to_owned(), call_id.clone());
    }
    out
}

fn json_string<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_string(value).ok()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
struct StoredChatMessage {
    id: Uuid,
    thread_id: Uuid,
    user_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    provider_id: String,
    model_id: String,
    model_name: String,
    role: String,
    thinking: Option<String>,
    content: String,
    tool_call: Option<String>,
    tool_result: Option<String>,
    created_at: i64,
    updated_at: i64,
    is_done: bool,
    usage: Option<String>,
}

impl StoredChatMessage {
    fn from_domain(thread_id: Uuid, message: ChatMessage, now: i64) -> Self {
        let updated_at = if message.updated_at == message.created_at {
            now
        } else {
            message.updated_at
        };
        Self {
            id: message.id,
            thread_id,
            user_id: message.user_id,
            parent_id: message.parent_id,
            provider_id: message.provider_id,
            model_id: message.model_id,
            model_name: message.model_name,
            role: match message.role {
                TurnRole::System => "system",
                TurnRole::User => "user",
                TurnRole::Assistant => "assistant",
                TurnRole::Tool => "tool",
            }
            .to_owned(),
            thinking: message.thinking,
            content: message.content,
            tool_call: message.tool_call,
            tool_result: message.tool_result,
            created_at: message.created_at,
            updated_at,
            is_done: message.is_done,
            usage: message.usage,
        }
    }

    fn from_row(row: chat_schema::chat_messages::Row) -> ChatMessage {
        ChatMessage {
            id: row.id,
            thread_id: row.thread_id,
            user_id: row.user_id,
            parent_id: row.parent_id,
            provider_id: row.provider_id,
            model_id: row.model_id,
            model_name: row.model_name,
            role: match row.role.as_str() {
                "system" => TurnRole::System,
                "user" => TurnRole::User,
                "tool" => TurnRole::Tool,
                _ => TurnRole::Assistant,
            },
            thinking: row.thinking,
            content: row.content,
            tool_call: row.tool_call,
            tool_result: row.tool_result,
            created_at: row.created_at,
            updated_at: row.updated_at,
            is_done: row.is_done,
            usage: row.usage,
        }
    }
}

#[allow(non_upper_case_globals)]
mod chat_schema {
    use super::*;

    migrations! {
        chat_schema {
            table chat_threads {
                id: Uuid [PrimaryKey],
                user_id: Option<Uuid>,
                title: String,
                created_at: i64,
                updated_at: i64,
                preview_text: String,
                is_pinned: bool,
            }

            table chat_messages {
                id: Uuid [PrimaryKey],
                thread_id: Uuid,
                user_id: Option<Uuid>,
                parent_id: Option<Uuid>,
                provider_id: String,
                model_id: String,
                model_name: String,
                role: String,
                thinking: Option<String>,
                content: String,
                tool_call: Option<String>,
                tool_result: Option<String>,
                created_at: i64,
                updated_at: i64,
                is_done: bool,
                usage: Option<String>,

                foreign_key(thread_id -> chat_threads.id);
            }
        }
    }
}
