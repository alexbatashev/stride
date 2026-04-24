use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

pub mod files;

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
}

/// The result of a tool execution
#[derive(Debug, Serialize, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(content: Value) -> Self {
        Self {
            success: true,
            content,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            content: Value::Null,
            error: Some(message.into()),
        }
    }
}

/// JSON Schema property for a parameter
#[derive(Debug, Clone, Serialize, Default)]
pub struct JsonSchemaProperty {
    #[serde(rename = "type")]
    pub property_type: String,
    pub description: String,
}

/// Tool definition for OpenAI-compatible function calling
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: ParametersDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParametersDefinition {
    #[serde(rename = "type")]
    pub param_type: String,
    pub properties: HashMap<String, JsonSchemaProperty>,
    pub required: Vec<String>,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters: ParametersDefinition {
                    param_type: "object".to_string(),
                    properties: HashMap::new(),
                    required: Vec::new(),
                },
            },
        }
    }

    pub fn with_param(
        mut self,
        name: impl Into<String>,
        param_type: impl Into<String>,
        description: impl Into<String>,
        required: bool,
    ) -> Self {
        let name = name.into();
        self.function.parameters.properties.insert(
            name.clone(),
            JsonSchemaProperty {
                property_type: param_type.into(),
                description: description.into(),
            },
        );
        if required {
            self.function.parameters.required.push(name);
        }
        self
    }
}

/// A tool call from the LLM
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    /// Parse the arguments JSON string into a Value
    pub fn parsed_arguments(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_str(&self.function.arguments)
    }
}

/// The tool trait - implement this to add new tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name (used for registration)
    fn name(&self) -> &str;

    /// Get the tool definition for the LLM (OpenAI function format)
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given arguments
    async fn execute(&self, args: Value, context: &ToolContext) -> ToolResult;

    /// Whether this tool requires confirmation before execution
    fn requires_confirmation(&self) -> bool {
        false
    }

    /// Get a description of what this tool will do (for confirmation prompts)
    fn confirmation_prompt(&self, args: &Value) -> String {
        format!("Execute {} with args: {}", self.name(), args)
    }
}

/// Tool registry for looking up tools by name
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a new tool
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Box::new(tool));
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Get all tool definitions for the LLM
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    /// Check if a tool requires confirmation
    pub fn requires_confirmation(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .map(|t| t.requires_confirmation())
            .unwrap_or(false)
    }

    /// Get confirmation prompt for a tool
    pub fn confirmation_prompt(&self, name: &str, args: &Value) -> Option<String> {
        self.tools.get(name).map(|t| t.confirmation_prompt(args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct TestTool;

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            "test_tool"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("test_tool", "A test tool").with_param(
                "input",
                "string",
                "The input",
                true,
            )
        }

        async fn execute(&self, _args: Value, _context: &ToolContext) -> ToolResult {
            ToolResult::success(Value::String("done".to_string()))
        }
    }

    #[test]
    fn test_tool_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(TestTool);

        assert!(registry.get("test_tool").is_some());
        assert!(registry.get("nonexistent").is_none());

        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
    }
}
