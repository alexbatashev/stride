use std::sync::Arc;

use crate::AgentConfig;
use crate::Tool;
use crate::ToolDesc;
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::Serialize;
use serde_json::Value;

pub struct GlobTool;

#[derive(ToolDesc)]
struct GlobParams {
    /// A glob pattern following syntax of libc glob function. Use this function
    /// to inspect repositories and directories to find relevant files.
    ///
    /// Examples:
    /// "*" - list all files in root directory
    /// "docs/**/*.md" - list all Markdown files in docs directory.
    pattern: String,
}

#[derive(Serialize)]
struct GlobResult {
    filename: String,
    size: u64,
}

#[async_trait(?Send)]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn readable_name(&self) -> &str {
        "Glob"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "todo".to_string(),
                name: self.name().to_owned(),
                parameters: Some(GlobParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let args = GlobParams::decode(args).unwrap();

        let mut entries = vec![];

        for path in glob::glob(&args.pattern).unwrap().flatten() {
            entries.push(GlobResult {
                filename: path.to_str().unwrap().to_string(),
                size: path.metadata().unwrap().len(),
            })
        }

        serde_json::to_value(entries).unwrap()
    }
}
