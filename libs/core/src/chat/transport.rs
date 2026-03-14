use crate::chat::{ChatMessage, LangModel, TurnRole, now_millis};
use crate::tools::Tool;

use super::tool_calls::{ModelFunctionCall, json_string};

use async_stream::stream;
use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use llm::{
    API, Anthropic, Completion, CompletionChoice, CompletionRequest, Message, Ollama, OpenAI, Role,
    UnnamedToolChoice,
};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct ChatProviderConfiguration {
    pub id: Uuid,
    pub name: String,
    pub kind: ChatProviderKind,
    pub base_url: String,
    pub token: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, uniffi::Enum)]
pub enum ChatProviderKind {
    OpenAICompatible,
    Ollama,
    Anthropic,
    Mock,
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
        let thread_id = messages
            .last()
            .map(|m| m.thread_id)
            .unwrap_or_else(Uuid::new_v4);
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
                created_at_ms: now,
                updated_at_ms: now,
                is_done: false,
                usage: None,
            };

            futures::pin_mut!(stream);
            while let Some(item) = stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        response.is_done = true;
                        response.updated_at_ms = now_millis();
                        yield Ok(response.clone());
                        yield Err(error);
                        return;
                    }
                };

                for choice in chunk.choices {
                    merge_tool_calls(&mut tool_calls, &choice);
                    if !tool_calls.is_empty() {
                        response.tool_call = json_string(&tool_calls);
                        response.updated_at_ms = now_millis();
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
                        response.updated_at_ms = now_millis();
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
                        response.updated_at_ms = now_millis();
                        yield Ok(response.clone());
                    }
                }
            }

            response.is_done = true;
            response.updated_at_ms = now_millis();
            yield Ok(response);
        })
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
