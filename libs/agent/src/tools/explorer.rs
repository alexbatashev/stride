use crate::{Tool, ToolRegistry};

use super::{file::ReadFileTool, glob::GlobTool, subagent::SubAgentTool};

pub const EXPLORER_MODEL: &str = "explorer";

pub fn make_explorer() -> SubAgentTool {
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(GlobTool {});
    tool_registry.allow_tool((GlobTool {}).name());
    tool_registry.register(ReadFileTool {});
    tool_registry.allow_tool((ReadFileTool {}).name());

    SubAgentTool::new(
        "explorer",
        "Workspace explorer",
        "A workspace explorer tool. Use this tool to get a basic idea of what the project looks like. Formulate your initial prompt to guide agent to discover project properties or input data required to complete your task. Always instruct agents to output a detailed summary containing those details as their last message",
        EXPLORER_MODEL,
        tool_registry,
    )
}
