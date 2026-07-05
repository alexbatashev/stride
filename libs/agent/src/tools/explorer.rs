use crate::ToolRegistry;

use super::subagent::FixedModelSubAgentTool;

pub const EXPLORER_MODEL: &str = "explorer";
pub const EXPLORER_NAME: &str = "explorer";

const SYSTEM_PROMPT: &str = "You are a workspace explorer agent. Use available tools to inspect only the context needed to answer the prompt. Finish with a detailed summary.

Return a self-contained answer the main agent can act on.";

pub fn make_explorer() -> FixedModelSubAgentTool {
    let mut registry = ToolRegistry::new();
    registry.register(super::glob::GlobTool);
    registry.register(super::file::ReadFileTool);

    FixedModelSubAgentTool::new(
        EXPLORER_NAME,
        "Workspace explorer",
        "A workspace explorer tool. Use this tool to get a basic idea of what the project looks like. Formulate your initial prompt to guide agent to discover project properties or input data required to complete your task. Always instruct agents to output a detailed summary containing those details as their last message.",
        EXPLORER_MODEL,
        SYSTEM_PROMPT,
        registry,
    )
}
