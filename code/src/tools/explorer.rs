use crate::tools::base_agent::BaseAgent;
use crate::tools::files::{ListFilesTool, ReadFileTool};
use crate::tools::{Tool, ToolContext, ToolDefinition, ToolResult};
use async_trait::async_trait;
use serde_json::{Value, json};

const EXPLORER_SYSTEM_PROMPT: &str = r#"You are a code context explorer. Investigate the codebase and produce a compressed summary.

Use list_files and read_file to:
1. List top-level directory structure
2. Read key config files (Cargo.toml, package.json, README, etc.)
3. Explore deeper directories based on what you find
4. Read main entry points and key modules
5. Follow imports to map dependencies

Output a concise structured summary covering:
- Project type, language, and purpose
- Directory/module structure with brief descriptions
- Key types, traits, and data structures
- Important functions and their roles
- External dependencies and their usage
- Architecture patterns (if notable)

Be thorough but extremely concise. Use bullet points. Target 200-500 words."#;

const EXPLORER_MAX_ITERATIONS: usize = 10;

pub struct ContextExplorerTool;

#[async_trait]
impl Tool for ContextExplorerTool {
    fn name(&self) -> &str {
        "explore_context"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "explore_context",
            "Explore the project codebase and return a compressed summary of its structure, key types, and architecture. Use this to understand the codebase before performing complex tasks.",
        )
        .with_param(
            "prompt",
            "string",
            "What to explore or understand about the codebase",
            true,
        )
    }

    async fn execute(&self, args: Value, context: &ToolContext) -> ToolResult {
        let prompt = match args["prompt"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("Missing required parameter 'prompt'"),
        };

        let mut runner = BaseAgent::new(
            context.api.clone(),
            context.token.clone(),
            context.model.clone(),
            context.clone(),
        );
        runner.register_tool(ReadFileTool);
        runner.register_tool(ListFilesTool);
        runner.set_max_iterations(EXPLORER_MAX_ITERATIONS);
        runner.set_thinking(context.thinking.clone());
        runner.set_system_prompt(EXPLORER_SYSTEM_PROMPT.to_string());

        match runner.run(prompt.to_string()).await {
            Ok(summary) => ToolResult::success(json!({ "summary": summary })),
            Err(e) => ToolResult::error(format!("Context exploration failed: {}", e)),
        }
    }
}
