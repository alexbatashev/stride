use crate::{Tool, ToolRegistry};

use super::{file::ReadFileTool, glob::GlobTool, subagent::SubAgentTool};

pub const EXPLORER_MODEL: &str = "explorer";

const SYSTEM_PROMPT: &str = "You are a workspace explorer agent. Use available tools to inspect only the context needed to answer the prompt. Finish with a detailed summary.
Target 200-500 words unless instructed otherwise. Only use bullets as formatting, no markdown or other markup languages. Be precise. Conserve output tokens.";

pub fn make_explorer() -> SubAgentTool {
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(GlobTool {});
    tool_registry.allow_tool((GlobTool {}).name());
    tool_registry.register(ReadFileTool {});
    tool_registry.allow_tool((ReadFileTool {}).name());

    SubAgentTool::new(
        "explorer",
        "Workspace explorer",
        "A workspace explorer tool. Use this tool to get a basic idea of what the project looks like. Formulate your initial prompt to guide agent to discover project properties or input data required to complete your task. Always instruct agents to output a detailed summary containing those details as their last message.",
        EXPLORER_MODEL,
        SYSTEM_PROMPT,
        tool_registry,
    )
}
