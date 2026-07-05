use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::Tool;
use llm::{FunctionParameters, FunctionProperty, Tool as LlmTool};
use serde_json::Value;

/// Reserved argument name the model sets to run an async-capable tool in the
/// background. Stripped before the tool sees its arguments.
pub const ASYNC_ARG: &str = "async";

const ASYNC_ARG_DESCRIPTION: &str = "Set to true to run this tool in the background and keep working \
     in the meantime; its result is delivered to you when it finishes. Set it \
     only when you do not need the result immediately.";

/// Returns the tool's LLM definition, injecting an `async` boolean parameter
/// when the tool supports background execution so the model can request it.
fn definition_with_async(tool: &Arc<dyn Tool>) -> LlmTool {
    let mut definition = tool.definition();
    if !tool.supports_async() {
        return definition;
    }
    let parameters = definition
        .function
        .parameters
        .get_or_insert_with(|| FunctionParameters {
            param_type: "object".to_string(),
            ..Default::default()
        });
    parameters.properties.insert(
        ASYNC_ARG.to_string(),
        FunctionProperty {
            r#type: "boolean".to_string(),
            description: ASYNC_ARG_DESCRIPTION.to_string(),
            extra: Default::default(),
        },
    );
    definition
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    searchable: HashMap<String, Arc<dyn Tool>>,
    allowed_tools: HashSet<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            searchable: HashMap::new(),
            allowed_tools: HashSet::new(),
        }
    }

    /// Register a new tool
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Register a tool that is hidden from the LLM by default and only
    /// discoverable via the search_tools tool.
    pub fn register_searchable<T: Tool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.searchable.insert(name, Arc::new(tool));
    }

    /// Mark tool as unconditionally safe
    pub fn allow_tool(&mut self, name: &str) {
        self.allowed_tools.insert(name.to_string());
    }

    /// Get a tool by name (searches both primary and searchable tools)
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .get(name)
            .or_else(|| self.searchable.get(name))
            .cloned()
    }

    /// Get all tool definitions for the LLM
    pub fn definitions(&self) -> Vec<LlmTool> {
        self.tools.values().map(definition_with_async).collect()
    }

    /// Tools (primary and searchable) that can be invoked without interactive
    /// approval. Used to advertise the agent's tools inside the Python sandbox,
    /// where mid-execution approval is not available.
    pub fn auto_approved(&self) -> Vec<Arc<dyn Tool>> {
        self.tools
            .values()
            .chain(self.searchable.values())
            .filter(|tool| {
                self.allowed_tools.contains(tool.name()) || !tool.requires_confirmation()
            })
            .cloned()
            .collect()
    }

    /// Check if a tool requires confirmation
    pub fn requires_confirmation(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .or_else(|| self.searchable.get(name))
            .map(|t| t.requires_confirmation())
            .unwrap_or(false)
    }

    /// Returns true when the tool needs approval from the user before execution
    pub fn needs_approval(&self, name: &str, args: &Value) -> bool {
        if self.allowed_tools.contains(name) {
            return false;
        }

        self.tools
            .get(name)
            .or_else(|| self.searchable.get(name))
            .map(|t| t.requires_confirmation() || !t.are_safe_args(args))
            .unwrap_or_default()
    }
}
