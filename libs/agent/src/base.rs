use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::sync::Arc;
use std::{cell::RefCell, rc::Rc};

use async_stream::stream;
use futures::channel::oneshot;
use futures::{Stream, StreamExt};
use llm::{API, CompletionRequest, Message, StreamResponseChunk, ToolCallChunk, ToolCallFunction};
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

use crate::{Tool, ToolRegistry};

pub const DEFAULT_MODEL: &str = "default";

pub struct BaseAgent(Rc<RefCell<BaseAgentInner>>);

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Missing model designated as '{0}'")]
    MissingProvider(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

impl From<llm::Error> for AgentError {
    fn from(err: llm::Error) -> Self {
        AgentError::NetworkError(err.to_string())
    }
}

pub enum AgentResponseChunk {
    Chunk(StreamResponseChunk),
    Approval {
        message: String,
        approved: oneshot::Sender<bool>,
    },
}

#[derive(Debug)]
pub struct AgentConfig {
    pub model_registry: ModelRegistry,
}

#[derive(Clone, Debug)]
pub struct ModelRegistry {
    models: HashMap<String, ModelRegEntry>,
}

#[derive(Clone, Debug)]
pub struct ModelRegEntry {
    pub api: API,
    pub token: String,
    pub model_name: String,
    pub thinking: bool,
}

struct BaseAgentInner {
    tool_registry: ToolRegistry,
    thread: Vec<Message>,
    config: Arc<AgentConfig>,
    model: String,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
        }
    }

    pub fn add_model(&mut self, name: &str, entry: ModelRegEntry) {
        self.models.insert(name.to_string(), entry);
    }

    pub fn get_or_default(&self, name: &str) -> &ModelRegEntry {
        if let Some(entry) = self.models.get(name) {
            entry
        } else if let Some(entry) = self.models.get(DEFAULT_MODEL) {
            entry
        } else {
            panic!("ModelRegistry must always have a 'default' model");
        }
    }
}

impl BaseAgent {
    pub fn new(model: String, config: Arc<AgentConfig>, thread: Vec<Message>) -> Self {
        Self(Rc::new(RefCell::new(BaseAgentInner {
            tool_registry: ToolRegistry::new(),
            thread,
            config,
            model,
        })))
    }

    pub fn register_tool(&self, tool: impl Tool + 'static) {
        self.0.borrow_mut().tool_registry.register(tool);
    }

    pub fn allow_tool(&self, name: &str) {
        self.0.borrow_mut().tool_registry.allow_tool(name);
    }

    pub async fn make_turn(
        &self,
        request: String,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>> {
        let (config, model) = {
            let lock = self.0.borrow();
            let model_name = &lock.model;
            (
                lock.config.clone(),
                lock.config
                    .model_registry
                    .get_or_default(model_name)
                    .clone(),
            )
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
                        model: model.model_name.clone(),
                        messages: lock.thread.clone(),
                        stream: Some(true),
                        tools: (!tools.is_empty()).then_some(tools.clone()),
                        ..Default::default()
                    }
                };

                let mut stream = model.api.stream_completion(&model.token, request);
                let mut tool_calls = BTreeMap::new();

                while let Some(chunk) = stream.next().await {
                    let is_err = chunk.is_err();
                    if let Ok(ref chunk) = chunk {
                        append_chunk(&agent, chunk, &mut tool_calls);
                    }
                    match chunk {
                        Ok(chunk) => { yield Ok(AgentResponseChunk::Chunk(chunk)); },
                        Err(err) => { yield Err(AgentError::from(err)); },
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
                        Some(tool) => {
                            let needs_approval = {
                                let lock = agent.borrow();
                                lock.tool_registry.needs_approval(&name, &args)
                            };

                            if needs_approval {
                                let message = tool.confirmation_prompt(&args);
                                let (approved, response) = oneshot::channel();
                                yield Ok(AgentResponseChunk::Approval { message, approved });

                                if !response.await.unwrap_or(false) {
                                    append_tool_result(
                                        &agent,
                                        id,
                                        json!({ "error": "tool execution denied by user" }),
                                    );
                                    continue;
                                }
                            }

                            tool.execute(config.clone(), args).await
                        }
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

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use futures::{StreamExt, pin_mut};
    use llm::{
        CompletionChoice, Delta, Function, FunctionParameters, StreamResponseChunk, ToolCallChunk,
        ToolCallFunction,
    };

    use super::*;

    #[derive(Clone)]
    struct ApprovalTool {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait(?Send)]
    impl Tool for ApprovalTool {
        fn name(&self) -> &str {
            "approval_tool"
        }

        fn readable_name(&self) -> &str {
            "Approval tool"
        }

        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "Test approval tool".to_string(),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }

        async fn execute(&self, _config: Arc<AgentConfig>, _args: Value) -> Value {
            self.calls.fetch_add(1, Ordering::SeqCst);
            json!({ "success": true })
        }

        fn requires_confirmation(&self) -> bool {
            true
        }

        fn confirmation_prompt(&self, args: &Value) -> String {
            format!("Approve approval_tool with {args}")
        }
    }

    fn make_agent(mock: &llm::Mock, calls: Arc<AtomicUsize>) -> BaseAgent {
        let mut registry = ModelRegistry::new();
        registry.add_model(
            DEFAULT_MODEL,
            ModelRegEntry {
                api: mock.clone().into(),
                token: String::new(),
                model_name: "mock-model".to_string(),
                thinking: false,
            },
        );
        let agent = BaseAgent::new(
            DEFAULT_MODEL.to_string(),
            Arc::new(AgentConfig { model_registry: registry }),
            vec![],
        );
        agent.register_tool(ApprovalTool { calls });
        agent
    }

    #[test]
    fn waits_for_approval_before_executing_tool() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = make_agent(&mock, calls.clone());

            let stream = agent.make_turn("run tool".to_string()).await;
            pin_mut!(stream);

            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                AgentResponseChunk::Chunk(_)
            ));

            match stream.next().await.unwrap().unwrap() {
                AgentResponseChunk::Approval { message, approved } => {
                    assert_eq!(message, r#"Approve approval_tool with {"value":1}"#);
                    approved.send(true).unwrap();
                }
                AgentResponseChunk::Chunk(_) => panic!("expected approval"),
            }

            assert_eq!(calls.load(Ordering::SeqCst), 0);

            let mut saw_done = false;
            while let Some(chunk) = stream.next().await {
                if chunk_text(&chunk.unwrap()) == Some("done") {
                    saw_done = true;
                }
            }

            assert!(saw_done);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(mock.stream_requests().len(), 2);
        });
    }

    #[test]
    fn denial_skips_tool_execution_and_reports_result() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let agent = make_agent(&mock, calls.clone());

            let stream = agent.make_turn("run tool".to_string()).await;
            pin_mut!(stream);

            stream.next().await.unwrap().unwrap();

            match stream.next().await.unwrap().unwrap() {
                AgentResponseChunk::Approval { approved, .. } => approved.send(false).unwrap(),
                AgentResponseChunk::Chunk(_) => panic!("expected approval"),
            }

            while stream.next().await.is_some() {}

            assert_eq!(calls.load(Ordering::SeqCst), 0);

            let requests = mock.stream_requests();
            let tool_result = requests[1]
                .messages
                .iter()
                .find(|message| message.role == llm::Role::Tool)
                .unwrap();
            assert_eq!(
                tool_result.content,
                r#"{"error":"tool execution denied by user"}"#
            );
        });
    }

    fn tool_call_chunk(arguments: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "chunk".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: None,
                text: None,
                index: 0,
                delta: Some(Delta {
                    content: None,
                    thinking: None,
                    tool_calls: Some(vec![ToolCallChunk {
                        index: Some(0),
                        id: Some("call_1".to_string()),
                        function: Some(ToolCallFunction {
                            name: Some("approval_tool".to_string()),
                            arguments: Some(arguments.to_string()),
                        }),
                    }]),
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("tool_calls".to_string()),
            }],
        }
    }

    fn text_chunk(content: &str) -> StreamResponseChunk {
        StreamResponseChunk {
            id: "chunk".to_string(),
            object: "mock.stream".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            system_fingerprint: None,
            choices: vec![CompletionChoice {
                message: None,
                text: Some(content.to_string()),
                index: 0,
                delta: Some(Delta {
                    content: Some(content.to_string()),
                    thinking: None,
                    tool_calls: None,
                }),
                logprobs: None,
                tool_calls: None,
                finish_reason: Some("stop".to_string()),
            }],
        }
    }

    fn chunk_text(chunk: &AgentResponseChunk) -> Option<&str> {
        let AgentResponseChunk::Chunk(chunk) = chunk else {
            return None;
        };

        chunk
            .choices
            .first()
            .and_then(|choice| choice.delta.as_ref())
            .and_then(|delta| delta.content.as_deref())
    }
}
