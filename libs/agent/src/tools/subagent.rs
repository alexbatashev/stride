use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde_json::Value;

use crate::{AgentConfig, Tool, ToolDesc, ToolRegistry};

pub struct SubAgentTool {
    name: String,
    readable_name: String,
    desc: String,
    tool_registry: ToolRegistry,
}

#[derive(ToolDesc)]
struct SubAgentParams {
    prompt: String,
}

impl SubAgentTool {
    pub fn new(name: &str, readable_name: &str, desc: &str, tool_registry: ToolRegistry) -> Self {
        Self {
            name: name.to_string(),
            readable_name: readable_name.to_string(),
            desc: desc.to_string(),
            tool_registry,
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
        todo!()
    }
}
