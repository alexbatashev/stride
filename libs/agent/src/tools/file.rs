use std::sync::Arc;

use crate::AgentConfig;
use crate::Tool;
use crate::ToolDesc;
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::Serialize;
use serde_json::{Value, json};

pub struct ReadFileTool;

#[derive(ToolDesc)]
struct ReadFileParams {
    /// Path to the UTF-8 text file to read.
    path: String,
}

#[derive(Serialize)]
struct ReadFileResult {
    success: bool,
    content: Option<String>,
    error: Option<String>,
}

#[async_trait(?Send)]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn readable_name(&self) -> &str {
        "Read file"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Read a UTF-8 text file.".to_string(),
                name: self.name().to_owned(),
                parameters: Some(ReadFileParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let result = match ReadFileParams::decode(args) {
            Ok(args) => std::fs::read_to_string(&args.path)
                .map_err(|err| format!("failed to read {}: {err}", args.path)),
            Err(error) => Err(error),
        };

        match result {
            Ok(content) => json!(ReadFileResult {
                success: true,
                content: Some(content),
                error: None,
            }),
            Err(error) => json!(ReadFileResult {
                success: false,
                content: None,
                error: Some(error),
            }),
        }
    }
}
