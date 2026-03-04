use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use friday::chat::{
    ChatMessage, ChatProviderConfiguration, ChatProviderKind, ChatService, ChatStorage,
    DirectChatTransport, NullChatStorage, TurnRole,
};
use friday::tools::{JSTool, Tool};
use futures::StreamExt;
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;

#[derive(Debug, Error, uniffi::Error)]
pub enum BridgeError {
    #[error("No models available for provider")]
    NoModelsAvailable,
    #[error("Chat stream failed: {message}")]
    ChatStreamFailed { message: String },
    #[error("Runtime initialization failed: {message}")]
    RuntimeInitFailed { message: String },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ChatModelInfo {
    pub provider: String,
    pub model: String,
    pub provider_name: String,
    pub model_name: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ChatReply {
    pub message_id: String,
    pub content: String,
    pub thinking: Option<String>,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum ProviderKind {
    OpenAICompatible,
    Ollama,
    Anthropic,
    Mock,
}

#[derive(uniffi::Object)]
pub struct ChatServiceBridge {
    runtime: Runtime,
    service: Arc<ChatService>,
    provider_id: String,
    thread_id: Uuid,
    js_tool: Arc<dyn Tool>,
}

#[uniffi::export]
impl ChatServiceBridge {
    #[uniffi::constructor]
    pub fn new(base_url: String, token: Option<String>) -> Result<Arc<Self>, BridgeError> {
        Self::with_provider(
            ProviderKind::Ollama,
            "Local Ollama".to_owned(),
            base_url,
            token,
        )
    }

    #[uniffi::constructor]
    pub fn with_provider(
        kind: ProviderKind,
        name: String,
        base_url: String,
        token: Option<String>,
    ) -> Result<Arc<Self>, BridgeError> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| BridgeError::RuntimeInitFailed {
                message: e.to_string(),
            })?;

        let provider = ChatProviderConfiguration {
            id: Uuid::new_v4(),
            name,
            kind: match kind {
                ProviderKind::OpenAICompatible => ChatProviderKind::OpenAICompatible,
                ProviderKind::Ollama => ChatProviderKind::Ollama,
                ProviderKind::Anthropic => ChatProviderKind::Anthropic,
                ProviderKind::Mock => ChatProviderKind::Mock,
            },
            base_url,
            token: token.unwrap_or_default(),
            default_model: String::new(),
        };

        let provider_id = provider.id.to_string();
        let transport = Arc::new(DirectChatTransport::from_provider(provider));
        let storage: Arc<dyn ChatStorage> = Arc::new(NullChatStorage);
        let service = Arc::new(ChatService::new(vec![transport], storage));

        Ok(Arc::new(Self {
            runtime,
            service,
            provider_id,
            thread_id: Uuid::new_v4(),
            js_tool: Arc::new(JSTool::new()),
        }))
    }

    pub fn list_models(&self) -> Vec<ChatModelInfo> {
        self.runtime.block_on(async {
            self.service
                .list_models()
                .await
                .into_iter()
                .map(|model| ChatModelInfo {
                    provider: model.provider,
                    model: model.model,
                    provider_name: model.provider_name,
                    model_name: model.model_name,
                })
                .collect()
        })
    }

    pub fn set_model(&self, model_id: String) {
        self.runtime.block_on(async {
            self.service
                .set_model(self.provider_id.clone(), model_id)
                .await;
        });
    }

    pub fn send_message(
        &self,
        prompt: String,
        tools_enabled: bool,
    ) -> Result<ChatReply, BridgeError> {
        self.runtime.block_on(async {
            let mut selected_model = self
                .service
                .list_models()
                .await
                .into_iter()
                .map(|m| m.model)
                .next();

            if selected_model.is_none() {
                return Err(BridgeError::NoModelsAvailable);
            }

            if let Some(model_id) = selected_model.take() {
                self.service
                    .set_model(self.provider_id.clone(), model_id.clone())
                    .await;

                let now = now_millis();
                let user_message = ChatMessage {
                    id: Uuid::new_v4(),
                    thread_id: self.thread_id,
                    user_id: None,
                    parent_id: None,
                    provider_id: self.provider_id.clone(),
                    model_id: model_id.clone(),
                    model_name: model_id,
                    role: TurnRole::User,
                    thinking: None,
                    content: prompt,
                    tool_call: None,
                    tool_result: None,
                    created_at: now,
                    updated_at: now,
                    is_done: false,
                    usage: None,
                };

                let tools = if tools_enabled {
                    vec![self.js_tool.clone()]
                } else {
                    vec![]
                };

                let mut stream = self.service.add_message(tools, user_message).await;
                let mut last: Option<ChatMessage> = None;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(msg) => last = Some(msg),
                        Err(error) => {
                            return Err(BridgeError::ChatStreamFailed {
                                message: error.to_string(),
                            });
                        }
                    }
                }

                let final_message = last.ok_or_else(|| BridgeError::ChatStreamFailed {
                    message: "No response chunks returned".to_owned(),
                })?;

                Ok(ChatReply {
                    message_id: final_message.id.to_string(),
                    content: final_message.content,
                    thinking: final_message.thinking,
                })
            } else {
                Err(BridgeError::NoModelsAvailable)
            }
        })
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

uniffi::setup_scaffolding!();
