use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use serde_json::{Value as JsonValue, json};
use std::sync::Arc;

use crate::vfs::{EntryKind, MountedVfs};

pub struct ListFilesTool {
    pub fs: MountedVfs,
}

pub struct ReadTextFileTool {
    pub fs: MountedVfs,
}

pub struct WriteTextFileTool {
    pub fs: MountedVfs,
}

#[derive(ToolDesc)]
struct ListFilesParams {
    /// Absolute path to list. Use "/" for the root, which contains your read-only files plus the writable "/~workspace" directory.
    path: String,
}

#[derive(ToolDesc)]
struct ReadTextFileParams {
    /// Absolute path to the file, e.g. "/notes.md" or "/~workspace/docs/notes.md".
    path: String,
}

#[derive(ToolDesc)]
struct WriteTextFileParams {
    /// Absolute path to write inside the workspace, e.g. "/~workspace/output.txt". Only paths under "/~workspace" are writable. Intermediate directories are created automatically.
    path: String,
    /// UTF-8 text content to write.
    content: String,
}

#[async_trait(?Send)]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn readable_name(&self) -> &str {
        "List Files"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "List files and directories at a given path. Use \"/\" to list the root, which contains your read-only files alongside the writable /~workspace directory.".to_string(),
                parameters: Some(ListFilesParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match ListFilesParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        match self.fs.list(&params.path).await {
            Ok(entries) => {
                let list: Vec<JsonValue> = entries
                    .iter()
                    .map(|e| {
                        let mut obj = json!({
                            "name": e.name,
                            "kind": match e.kind { EntryKind::Directory => "directory", EntryKind::File => "file" },
                        });
                        if let Some(size) = e.size {
                            obj["size"] = json!(size);
                        }
                        if let Some(mime) = &e.mime_type {
                            obj["mime_type"] = json!(mime);
                        }
                        obj
                    })
                    .collect();
                json!({"entries": list})
            }
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for ReadTextFileTool {
    fn name(&self) -> &str {
        "read_text_file"
    }

    fn readable_name(&self) -> &str {
        "Read File"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Read the text content of a file. Use an absolute path such as /notes.md or /~workspace/notes.md.".to_string(),
                parameters: Some(ReadTextFileParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match ReadTextFileParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        match self.fs.read(&params.path).await {
            Ok(content) => json!({"content": content}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for WriteTextFileTool {
    fn name(&self) -> &str {
        "write_text_file"
    }

    fn readable_name(&self) -> &str {
        "Write File"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Write text content to a file. Creates the file and any missing parent directories. Each write creates a new version; old versions beyond the configured limit are deleted.".to_string(),
                parameters: Some(WriteTextFileParams::function_parameters()),
            },
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &JsonValue) -> String {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        format!("Write file {path}")
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match WriteTextFileParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        match self.fs.write(&params.path, &params.content).await {
            Ok(()) => json!({"success": true, "path": params.path}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}
