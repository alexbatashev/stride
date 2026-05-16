use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

use crate::{AgentConfig, AgentResponseChunk, BaseAgent, Tool, ToolDesc, ToolRegistry};

pub struct SubAgentTool {
    name: String,
    readable_name: String,
    desc: String,
    tool_registry: ToolRegistry,
    model: String,
    system_prompt: String,
}

#[derive(ToolDesc)]
struct SubAgentParams {
    /// An initial prompt that will be used by sub-agent.
    prompt: String,
}

impl SubAgentTool {
    pub fn new(
        name: &str,
        readable_name: &str,
        desc: &str,
        model: &str,
        system_prompt: &str,
        tool_registry: ToolRegistry,
    ) -> Self {
        Self {
            name: name.to_string(),
            readable_name: readable_name.to_string(),
            desc: desc.to_string(),
            tool_registry,
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
        }
    }
}

#[async_trait(?Send)]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn readable_name(&self) -> &str {
        &self.readable_name
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: self.desc.clone(),
                name: self.name.clone(),
                parameters: Some(SubAgentParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: Value) -> Value {
        let args = match SubAgentParams::decode(args) {
            Ok(args) => args,
            Err(error) => return json!({ "success": false, "error": error }),
        };

        let agent = BaseAgent::new_with_tools(
            self.model.clone(),
            config,
            self.system_prompt.clone(),
            vec![],
            self.tool_registry.clone(),
        );
        let mut stream = agent.make_turn(args.prompt).await;
        let mut content = String::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(AgentResponseChunk::Chunk(chunk)) => {
                    for choice in chunk.choices {
                        if let Some(delta_content) = choice.delta.and_then(|d| d.content) {
                            content.push_str(&delta_content);
                        }
                    }
                }
                Ok(AgentResponseChunk::Approval { approved, .. }) => {
                    let _ = approved.send(false);
                }
                Ok(AgentResponseChunk::Quiz { answered, .. }) => {
                    let _ = answered.send(vec![]);
                }
                Err(error) => return json!({ "success": false, "error": error.to_string() }),
            }
        }

        json!({ "success": true, "content": content })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use llm::{
        CompletionChoice, Delta, Function, FunctionParameters, StreamResponseChunk, ToolCallChunk,
        ToolCallFunction,
    };

    use super::*;
    use crate::{DEFAULT_MODEL, ModelRegEntry, ModelRegistry};

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

        fn definition(&self) -> LlmTool {
            LlmTool {
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
    }

    #[test]
    fn returns_final_subagent_content() {
        futures::executor::block_on(async {
            let mock = llm::Mock::new()
                .with_stream_chunks(vec![vec![text_chunk("final "), text_chunk("answer")]]);
            let tool = SubAgentTool::new(
                "subagent",
                "Subagent",
                "Run a subagent.",
                DEFAULT_MODEL,
                "Return final content.",
                ToolRegistry::new(),
            );
            let result = tool
                .execute(config(&mock), json!({ "prompt": "inspect this" }))
                .await;

            assert_eq!(
                result,
                json!({ "success": true, "content": "final answer" })
            );
            let messages = &mock.stream_requests()[0].messages;
            assert_eq!(messages[0].role, llm::Role::System);
            assert_eq!(messages[0].content, "Return final content.");
            assert_eq!(messages[1].content, "inspect this");
        });
    }

    #[test]
    fn denies_inner_approval_requests() {
        futures::executor::block_on(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let mock = llm::Mock::new().with_stream_chunks(vec![
                vec![tool_call_chunk(r#"{"value":1}"#)],
                vec![text_chunk("done")],
            ]);
            let mut registry = ToolRegistry::new();
            registry.register(ApprovalTool {
                calls: calls.clone(),
            });
            let tool = SubAgentTool::new(
                "subagent",
                "Subagent",
                "Run a subagent.",
                DEFAULT_MODEL,
                "Never execute tools without approval.",
                registry,
            );

            let result = tool
                .execute(config(&mock), json!({ "prompt": "run inner tool" }))
                .await;

            assert_eq!(result, json!({ "success": true, "content": "done" }));
            assert_eq!(calls.load(Ordering::SeqCst), 0);

            let requests = mock.stream_requests();
            assert_eq!(requests[0].messages[0].role, llm::Role::System);
            assert_eq!(
                requests[0].messages[0].content,
                "Never execute tools without approval."
            );
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

    fn config(mock: &llm::Mock) -> Arc<AgentConfig> {
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
        Arc::new(AgentConfig {
            model_registry: registry,
            max_iterations: 50,
        })
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
}
