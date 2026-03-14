use super::ChatMessage;
use super::ChatThread;
use super::LangModel;
use super::ToolInvocation;
use super::ToolInvocationStatus;
use super::TurnRole;
use super::now_millis;
use super::storage::*;
use super::tool_calls::{
    extract_function_calls, json_string, parse_tool_args, tool_result_dictionary,
};
use super::transport::*;
use crate::futures::Stream;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_lock::Mutex;
use async_stream::stream;
use futures::StreamExt;
use llm::{
    API, Completion, CompletionChoice, CompletionRequest, Message, OpenAI, Role, UnnamedToolChoice,
};
use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::tools::{JSTool, Tool};

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

#[derive(Clone, uniffi::Object)]
pub struct ChatService {
    transports: Vec<Arc<dyn ChatTransport>>,
    storage: Arc<dyn ChatStorage>,
    // TODO: for now this is async Mutex, however we never hold lock across .await. Should we change this to std::sync::Mutex?
    state: Arc<Mutex<ChatServiceState>>,
}

#[derive(Debug, Default)]
struct ChatServiceState {
    provider_id: Option<String>,
    model_id: Option<String>,
    has_loaded_storage: bool,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct ToolsConfig {
    pub use_js: bool,
}

#[uniffi::export]
impl ChatService {
    #[uniffi::constructor]
    pub fn new(transports: Vec<Arc<dyn ChatTransport>>, storage: Arc<dyn ChatStorage>) -> Self {
        let mut transports = transports;
        transports.sort_by(|a, b| a.provider_id().cmp(b.provider_id()));
        Self {
            transports,
            storage,
            state: Arc::new(Mutex::new(ChatServiceState::default())),
        }
    }

    pub async fn add_message(
        &self,
        tools: ToolsConfig,
        next: ChatMessage,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatMessage, ChatStreamError>> + Send + 'static>> {
        let this = self.clone();
        self.ensure_messages_loaded().await;
        self.append_message(next.clone()).await;

        let tools = create_tools(tools);

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
                                latest.updated_at_ms = now_millis();
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
                assistant.updated_at_ms = now_millis();
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
                        created_at_ms: now_millis(),
                        updated_at_ms: now_millis(),
                        is_done: true,
                        usage: None,
                    };
                    this.append_message(tool_message.clone()).await;
                    working_messages.push(tool_message);
                }

                assistant.tool_result = json_string(&tool_result_entries);
                assistant.updated_at_ms = now_millis();
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
        loaded.sort_by_key(|message| message.created_at_ms);
        self.state.lock().await.messages = loaded;
    }

    async fn append_message(&self, message: ChatMessage) {
        self.state.lock().await.messages.push(message.clone());
        self.storage.append_message(message).await;
    }
}

#[uniffi::export]
impl ChatService {
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
        state.provider_id = Some(provider_id.to_lowercase());
        state.model_id = Some(model_id);
    }
}

fn create_tools(config: ToolsConfig) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    if config.use_js {
        tools.push(Arc::new(JSTool::new()));
    }
    tools
}
