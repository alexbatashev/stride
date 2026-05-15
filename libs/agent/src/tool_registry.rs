use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::Tool;
use llm::Tool as LlmTool;
use serde_json::Value;

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
        self.tools.values().map(|t| t.definition()).collect()
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
