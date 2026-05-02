use std::collections::BTreeMap;
use std::pin::Pin;
use std::{cell::RefCell, rc::Rc};

use async_stream::stream;
use futures::channel::oneshot;
use futures::{Stream, StreamExt};
use llm::{API, CompletionRequest, Message, StreamResponseChunk, ToolCallChunk, ToolCallFunction};
use serde_json::Value;
use serde_json::json;

use crate::{Tool, ToolRegistry};

pub struct BaseAgent(Rc<RefCell<BaseAgentInner>>);

pub enum AgentResponseChunk {
    Chunk(StreamResponseChunk),
    Approval {
        message: String,
        approved: oneshot::Sender<bool>,
    },
}

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub model: String,
    pub thinking: bool,
}

struct BaseAgentInner {
    api: API,
    token: String,
    tool_registry: ToolRegistry,
    thread: Vec<Message>,
    config: AgentConfig,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl BaseAgent {
    pub fn new(api: API, token: String, config: AgentConfig, thread: Vec<Message>) -> Self {
        Self(Rc::new(RefCell::new(BaseAgentInner {
            api,
            token,
            tool_registry: ToolRegistry::new(),
            thread,
            config,
        })))
    }

    pub fn register_tool(&self, tool: impl Tool + 'static) {
        self.0.borrow_mut().tool_registry.register(tool);
    }

    pub async fn make_turn(
        &self,
        request: String,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, llm::Error>> + 'static>> {
        let (api, token, config) = {
            let lock = self.0.borrow();
            (lock.api.clone(), lock.token.clone(), lock.config.clone())
        };

        self.0.borrow_mut().thread.push(Message {
            role: llm::Role::User,
            content: request,
            ..Default::default()
        });

        let tools = self.0.borrow().tool_registry.definitions();
        let agent = self.0.clone();

        Box::pin(stream! {
            loop {
                {
                    agent.borrow_mut().thread.push(Message {
                        role: llm::Role::Assistant,
                        content: String::new(),
                        ..Default::default()
                    });
                }

                let request = {
                    let lock = agent.borrow();
                    CompletionRequest {
                        model: config.model.clone(),
                        messages: lock.thread.clone(),
                        stream: Some(true),
                        tools: (!tools.is_empty()).then_some(tools.clone()),
                        ..Default::default()
                    }
                };

                let mut stream = api.stream_completion(&token, request);
                let mut tool_calls = BTreeMap::new();

                while let Some(chunk) = stream.next().await {
                    let is_err = chunk.is_err();
                    if let Ok(ref chunk) = chunk {
                        append_chunk(&agent, chunk, &mut tool_calls);
                    }
                    match chunk {
                        Ok(chunk) => { yield Ok(AgentResponseChunk::Chunk(chunk)); },
                        Err(err) => { yield Err(err); },
                    }
                    if is_err {
                        return;
                    }
                }

                let tool_calls = finish_tool_calls(&agent, tool_calls);
                if tool_calls.is_empty() {
                    break;
                }

                for (id, name, arguments) in tool_calls {
                    let args = match serde_json::from_str::<Value>(&arguments) {
                        Ok(args) => args,
                        Err(err) => {
                            append_tool_result(&agent, id, json!({ "error": err.to_string() }));
                            continue;
                        }
                    };

                    let tool = {
                        let lock = agent.borrow();
                        lock.tool_registry.get(&name)
                    };

                    let result = match tool {
                        Some(tool) => tool.execute(args).await,
                        None => json!({ "error": format!("unknown tool: {}", name) }),
                    };

                    append_tool_result(&agent, id, result);
                }
            }
        })
    }
}

fn append_chunk(
    agent: &Rc<RefCell<BaseAgentInner>>,
    chunk: &StreamResponseChunk,
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
) {
    let mut lock = agent.borrow_mut();
    let message = lock.thread.last_mut().unwrap();

    for choice in &chunk.choices {
        if let Some(delta) = &choice.delta {
            if let Some(content) = &delta.content {
                message.content.push_str(content);
            }

            if let Some(thinking) = &delta.thinking {
                message
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(thinking);
            }

            if let Some(chunks) = &delta.tool_calls {
                for chunk in chunks {
                    let index = chunk.index.unwrap_or(0);
                    let call = tool_calls.entry(index).or_default();

                    if let Some(id) = &chunk.id {
                        call.id.push_str(id);
                    }

                    if let Some(function) = &chunk.function {
                        if let Some(name) = &function.name {
                            call.name.push_str(name);
                        }
                        if let Some(arguments) = &function.arguments {
                            call.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }
    }
}

fn finish_tool_calls(
    agent: &Rc<RefCell<BaseAgentInner>>,
    tool_calls: BTreeMap<usize, PartialToolCall>,
) -> Vec<(String, String, String)> {
    let tool_calls: Vec<_> = tool_calls
        .into_values()
        .filter(|call| !call.name.is_empty())
        .map(|call| (call.id, call.name, call.arguments))
        .collect();

    if tool_calls.is_empty() {
        return tool_calls;
    }

    let mut lock = agent.borrow_mut();
    let message = lock.thread.last_mut().unwrap();
    message.tool_calls = Some(
        tool_calls
            .iter()
            .map(|(id, name, arguments)| ToolCallChunk {
                index: None,
                id: Some(id.clone()),
                function: Some(ToolCallFunction {
                    name: Some(name.clone()),
                    arguments: Some(arguments.clone()),
                }),
            })
            .collect(),
    );

    tool_calls
}

fn append_tool_result(agent: &Rc<RefCell<BaseAgentInner>>, id: String, result: Value) {
    let content =
        serde_json::to_string(&result).unwrap_or_else(|err| format!(r#"{{"error":"{}"}}"#, err));

    agent.borrow_mut().thread.push(Message {
        role: llm::Role::Tool,
        content,
        tool_call_id: Some(id),
        ..Default::default()
    });
}
