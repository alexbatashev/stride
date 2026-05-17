use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::{cell::RefCell, rc::Rc};

use async_stream::stream;
use futures::channel::oneshot;
use futures::{Stream, StreamExt};
use llm::{API, CompletionRequest, Message, StreamResponseChunk, ToolCallChunk, ToolCallFunction};
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

use crate::tools::search::{SearchEntry, SearchTool};
use crate::{QuizQuestion, Tool, ToolRegistry};

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
        tool_name: String,
        message: String,
        approved: oneshot::Sender<bool>,
    },
    Quiz {
        tool_name: String,
        questions: Vec<QuizQuestion>,
        answered: oneshot::Sender<Vec<String>>,
    },
}

#[derive(Debug)]
pub struct AgentConfig {
    pub model_registry: ModelRegistry,
    pub max_iterations: usize,
}

#[derive(Clone, Debug, Default)]
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
    tool_display_names: HashMap<usize, String>,
    config: Arc<AgentConfig>,
    model: String,
    search_slot: Arc<Mutex<Vec<llm::Tool>>>,
    searchable_entries: Arc<Mutex<Vec<SearchEntry>>>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
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
    pub fn new(
        model: String,
        config: Arc<AgentConfig>,
        system_prompt: String,
        thread: Vec<Message>,
    ) -> Self {
        Self::new_with_tools(model, config, system_prompt, thread, ToolRegistry::new())
    }

    pub fn new_with_tools(
        model: String,
        config: Arc<AgentConfig>,
        system_prompt: String,
        thread: Vec<Message>,
        tool_registry: ToolRegistry,
    ) -> Self {
        let thread = thread_with_system_prompt(system_prompt, thread);

        Self(Rc::new(RefCell::new(BaseAgentInner {
            tool_registry,
            thread,
            tool_display_names: HashMap::new(),
            config,
            model,
            search_slot: Arc::new(Mutex::new(Vec::new())),
            searchable_entries: Arc::new(Mutex::new(Vec::new())),
        })))
    }

    pub fn register_tool(&self, tool: impl Tool + 'static) {
        self.0.borrow_mut().tool_registry.register(tool);
    }

    /// Register a tool that is hidden from the LLM by default. The LLM can
    /// discover it via `search_tools` and use it for one turn.
    pub fn register_searchable_tool(&self, tool: impl Tool + 'static) {
        let mut lock = self.0.borrow_mut();
        let entry = SearchEntry {
            name: tool.name().to_string(),
            description: tool.definition().function.description.clone(),
            definition: tool.definition(),
        };
        lock.searchable_entries.lock().unwrap().push(entry);
        lock.tool_registry.register_searchable(tool);

        if lock.tool_registry.get("search_tools").is_none() {
            let search_tool = SearchTool {
                entries: lock.searchable_entries.clone(),
                slot: lock.search_slot.clone(),
            };
            lock.tool_registry.register(search_tool);
        }
    }

    pub fn allow_tool(&self, name: &str) {
        self.0.borrow_mut().tool_registry.allow_tool(name);
    }

    pub fn set_config(&self, config: Arc<AgentConfig>) {
        self.0.borrow_mut().config = config;
    }

    pub fn set_thread(&self, system_prompt: String, thread: Vec<Message>) {
        let mut lock = self.0.borrow_mut();
        lock.thread = thread_with_system_prompt(system_prompt, thread);
        lock.tool_display_names.clear();
    }

    pub fn thread(&self) -> Vec<Message> {
        self.0.borrow().thread.clone()
    }

    pub fn tool_display_names(&self) -> HashMap<usize, String> {
        self.0.borrow().tool_display_names.clone()
    }

    pub fn tool_definitions(&self) -> Vec<llm::Tool> {
        self.0.borrow().tool_registry.definitions()
    }

    pub async fn make_turn(
        &self,
        request: String,
    ) -> Pin<Box<dyn Stream<Item = Result<AgentResponseChunk, AgentError>> + 'static>> {
        let (config, model, max_iterations) = {
            let lock = self.0.borrow();
            let model_name = &lock.model;
            (
                lock.config.clone(),
                lock.config
                    .model_registry
                    .get_or_default(model_name)
                    .clone(),
                lock.config.max_iterations,
            )
        };

        self.0.borrow_mut().thread.push(Message {
            role: llm::Role::User,
            content: request,
            ..Default::default()
        });

        let agent = self.0.clone();

        Box::pin(stream! {
            for _ in 0..max_iterations {
                let tools = {
                    let lock = agent.borrow();
                    let mut t = lock.tool_registry.definitions();
                    let extra: Vec<_> = lock.search_slot.lock().unwrap().drain(..).collect();
                    t.extend(extra);
                    t
                };

                {
                    agent.borrow_mut().thread.push(Message {
                        role: llm::Role::Assistant,
                        content: String::new(),
                        ..Default::default()
                    });
                }

                let request = {
                    let lock = agent.borrow();
                    let mut request = CompletionRequest {
                        model: model.model_name.clone(),
                        messages: lock.thread.clone(),
                        stream: Some(true),
                        tools: (!tools.is_empty()).then_some(tools.clone()),
                        ..Default::default()
                    };
                    if model.thinking {
                        request.thinking = Some(llm::ThinkingConfig {
                            thinking_type: "native".to_string(),
                            budget_tokens: None,
                        });
                    }
                    request
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
                    return;
                }

                for (id, name, arguments) in tool_calls {
                    tracing::info!(tool = %name, arguments = %arguments, "tool call requested");

                    let args = match serde_json::from_str::<Value>(&arguments) {
                        Ok(args) => args,
                        Err(err) => {
                            let result = json!({ "error": err.to_string() });
                            log_tool_error(&name, &result);
                            append_tool_result(&agent, id, result, "tool error".to_string());
                            continue;
                        }
                    };

                    let tool = {
                        let lock = agent.borrow();
                        lock.tool_registry.get(&name)
                    };

                    let (result, readable_name) = match tool {
                        Some(tool) => {
                            let readable_name = tool.readable_name().to_string();

                            if let Some(questions) = tool.quiz_questions(&args) {
                                let (answered, response) = oneshot::channel();
                                yield Ok(AgentResponseChunk::Quiz {
                                    tool_name: readable_name.clone(),
                                    questions: questions.clone(),
                                    answered,
                                });
                                let answers = response.await.unwrap_or_default();
                                let result = json!({
                                    "answers": questions.iter().zip(answers.iter()).map(|(q, a)| {
                                        json!({ "question": q.question, "answer": a })
                                    }).collect::<Vec<_>>()
                                });
                                log_tool_result(&name, &result);
                                append_tool_result(&agent, id, result, readable_name);
                                continue;
                            }

                            let needs_approval = {
                                let lock = agent.borrow();
                                lock.tool_registry.needs_approval(&name, &args)
                            };

                            if needs_approval {
                                let message = tool.confirmation_prompt(&args);
                                let (approved, response) = oneshot::channel();
                                yield Ok(AgentResponseChunk::Approval { tool_name: readable_name.clone(), message, approved });

                                if !response.await.unwrap_or(false) {
                                    let result = json!({ "error": "tool execution denied by user" });
                                    log_tool_error(&name, &result);
                                    append_tool_result(&agent, id, result, readable_name);
                                    continue;
                                }
                            }

                            let result = tool.execute(config.clone(), args).await;
                            (result, readable_name)
                        }
                        None => (
                            json!({ "error": format!("unknown tool: {}", name) }),
                            name.clone(),
                        ),
                    };

                    log_tool_result(&name, &result);
                    append_tool_result(&agent, id, result, readable_name);
                }
            }

            yield Err(AgentError::NetworkError(format!(
                "reached maximum tool iteration limit ({max_iterations})"
            )));
        })
    }
}

fn log_tool_error(name: &str, result: &Value) {
    if let Some(error) = result.get("error") {
        tracing::error!(tool = %name, error = %error, "tool call failed");
    }
}

fn log_tool_result(name: &str, result: &Value) {
    if result.get("error").is_some() {
        log_tool_error(name, result);
    } else {
        tracing::info!(tool = %name, "tool call finished");
    }
}

fn thread_with_system_prompt(system_prompt: String, thread: Vec<Message>) -> Vec<Message> {
    let mut prompted_thread = Vec::with_capacity(thread.len() + 1);
    prompted_thread.push(Message {
        role: llm::Role::System,
        content: system_prompt,
        ..Default::default()
    });
    prompted_thread.extend(thread);
    prompted_thread
}

fn append_chunk(
    agent: &Rc<RefCell<BaseAgentInner>>,
    chunk: &StreamResponseChunk,
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
) {
    let mut lock = agent.borrow_mut();
    let message = lock.thread.last_mut().unwrap();

    for choice in &chunk.choices {
        if let Some(content) = &choice.text {
            message.content.push_str(content);
        }

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

fn append_tool_result(
    agent: &Rc<RefCell<BaseAgentInner>>,
    id: String,
    result: Value,
    readable_name: String,
) {
    let content =
        serde_json::to_string(&result).unwrap_or_else(|err| format!(r#"{{"error":"{}"}}"#, err));

    let mut lock = agent.borrow_mut();
    lock.thread.push(Message {
        role: llm::Role::Tool,
        content,
        tool_call_id: Some(id),
        ..Default::default()
    });
    let idx = lock.thread.len() - 1;
    lock.tool_display_names.insert(idx, readable_name);
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
            Arc::new(AgentConfig {
                model_registry: registry,
                max_iterations: 50,
            }),
            String::new(),
            vec![],
        );
        agent.register_tool(ApprovalTool { calls });
        agent
    }

    #[test]
    fn sends_system_prompt_before_thread_messages() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![vec![text_chunk("done")]]);
            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                "Use short answers.".to_string(),
                vec![Message {
                    role: llm::Role::User,
                    content: "previous".to_string(),
                    ..Default::default()
                }],
            );
            agent.register_tool(ApprovalTool { calls });

            let stream = agent.make_turn("next".to_string()).await;
            pin_mut!(stream);
            while stream.next().await.is_some() {}

            let requests = mock.stream_requests();
            assert_eq!(requests[0].messages[0].role, llm::Role::System);
            assert_eq!(requests[0].messages[0].content, "Use short answers.");
            assert_eq!(requests[0].messages[1].content, "previous");
            assert_eq!(requests[0].messages[2].content, "next");
        });
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
                AgentResponseChunk::Approval {
                    message, approved, ..
                } => {
                    assert_eq!(message, r#"Approve approval_tool with {"value":1}"#);
                    approved.send(true).unwrap();
                }
                AgentResponseChunk::Chunk(_) | AgentResponseChunk::Quiz { .. } => {
                    panic!("expected approval")
                }
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
            assert!(
                agent
                    .tool_display_names()
                    .values()
                    .any(|name| name == "Approval tool")
            );
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
                AgentResponseChunk::Chunk(_) | AgentResponseChunk::Quiz { .. } => {
                    panic!("expected approval")
                }
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

    fn registry(mock: &llm::Mock) -> ModelRegistry {
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
        registry
    }

    fn named_tool_call_chunk(name: &str, arguments: &str) -> StreamResponseChunk {
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
                            name: Some(name.to_string()),
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

    struct EchoTool;

    #[async_trait(?Send)]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn readable_name(&self) -> &str {
            "Echo"
        }

        fn definition(&self) -> llm::Tool {
            llm::Tool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: "Echo back the input message.".to_string(),
                    name: self.name().to_string(),
                    parameters: Some(FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }

        async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
            json!({ "echoed": args })
        }
    }

    #[test]
    fn search_tools_injects_found_tools_for_one_turn() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new().with_stream_chunks(vec![
                // Iteration 1: LLM calls search_tools
                vec![named_tool_call_chunk("search_tools", r#"{"query":"echo"}"#)],
                // Iteration 2: LLM uses the found echo tool
                vec![named_tool_call_chunk("echo", r#"{"msg":"hello"}"#)],
                // Iteration 3: LLM returns text
                vec![text_chunk("done")],
            ]);

            let agent = BaseAgent::new(
                DEFAULT_MODEL.to_string(),
                Arc::new(AgentConfig {
                    model_registry: registry(&mock),
                    max_iterations: 50,
                }),
                String::new(),
                vec![],
            );
            agent.register_searchable_tool(EchoTool);

            let stream = agent.make_turn("go".to_string()).await;
            pin_mut!(stream);
            while stream.next().await.is_some() {}

            let requests = mock.stream_requests();
            assert_eq!(requests.len(), 3);

            let tool_names = |req: &llm::CompletionRequest| -> Vec<String> {
                req.tools
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .map(|t| t.function.name.clone())
                    .collect()
            };

            // Iteration 1: only search_tools visible
            let names1 = tool_names(&requests[0]);
            assert!(names1.contains(&"search_tools".to_string()));
            assert!(!names1.contains(&"echo".to_string()));

            // Iteration 2: echo injected for this turn
            let names2 = tool_names(&requests[1]);
            assert!(names2.contains(&"search_tools".to_string()));
            assert!(names2.contains(&"echo".to_string()));

            // Iteration 3: echo gone again
            let names3 = tool_names(&requests[2]);
            assert!(names3.contains(&"search_tools".to_string()));
            assert!(!names3.contains(&"echo".to_string()));
        });
    }
}
