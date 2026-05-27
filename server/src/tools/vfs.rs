use std::sync::Arc;

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::vfs::{EntryKind, Vfs};

pub struct VfsListTool {
    pub vfs: Arc<Vfs>,
    pub workspace_id: Uuid,
}

pub struct VfsReadTool {
    pub vfs: Arc<Vfs>,
    pub workspace_id: Uuid,
}

pub struct VfsWriteTool {
    pub vfs: Arc<Vfs>,
    pub workspace_id: Uuid,
    pub owner: Uuid,
}

#[derive(ToolDesc)]
struct VfsListParams {
    /// Absolute path to list. Use "/" to see top-level directories, "/~workspace" to list the workspace root, or "/~workspace/subdir" for subdirectories.
    path: String,
}

#[derive(ToolDesc)]
struct VfsReadParams {
    /// Absolute path to the file, e.g. "/~workspace/notes.md".
    path: String,
}

#[derive(ToolDesc)]
struct VfsWriteParams {
    /// Absolute path to write, must start with "/~workspace/", e.g. "/~workspace/output.txt". Intermediate directories are created automatically.
    path: String,
    /// UTF-8 text content to write.
    content: String,
}

fn strip_workspace_prefix(path: &str) -> Option<&str> {
    let p = path.trim_start_matches('/');
    let p = p.strip_prefix("~workspace").unwrap_or(p);
    let p = p.trim_start_matches('/');
    Some(p)
}

#[async_trait(?Send)]
impl Tool for VfsListTool {
    fn name(&self) -> &str {
        "vfs_list"
    }

    fn readable_name(&self) -> &str {
        "List Files"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "List files and directories at a given path. Use \"/\" to see the workspace entry, \"/~workspace\" to list the workspace root.".to_string(),
                parameters: Some(VfsListParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match VfsListParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        // special-case root: show ~workspace as the only entry
        if params.path == "/" || params.path.is_empty() {
            return json!({
                "entries": [{"name": "~workspace", "kind": "directory"}]
            });
        }

        let rel = match strip_workspace_prefix(&params.path) {
            Some(p) => p,
            None => return json!({"error": "path must start with /~workspace"}),
        };

        match self.vfs.list(self.workspace_id, rel).await {
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
impl Tool for VfsReadTool {
    fn name(&self) -> &str {
        "vfs_read"
    }

    fn readable_name(&self) -> &str {
        "Read File"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Read the text content of a file in the workspace. Path must start with /~workspace/.".to_string(),
                parameters: Some(VfsReadParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match VfsReadParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let rel = match strip_workspace_prefix(&params.path) {
            Some(p) if !p.is_empty() => p,
            _ => return json!({"error": "path must point to a file inside /~workspace/"}),
        };

        match self.vfs.read(self.workspace_id, rel).await {
            Ok(content) => json!({"content": content}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

#[async_trait(?Send)]
impl Tool for VfsWriteTool {
    fn name(&self) -> &str {
        "vfs_write"
    }

    fn readable_name(&self) -> &str {
        "Write File"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Write text content to a file in the workspace. Path must start with /~workspace/. Creates the file and any missing parent directories. Each write creates a new version; old versions beyond the configured limit are deleted.".to_string(),
                parameters: Some(VfsWriteParams::function_parameters()),
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
        let params = match VfsWriteParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        if !params.path.contains("~workspace") {
            return json!({"error": "write access is restricted to /~workspace/"});
        }

        let rel = match strip_workspace_prefix(&params.path) {
            Some(p) if !p.is_empty() => p,
            _ => return json!({"error": "path must point to a file inside /~workspace/"}),
        };

        match self
            .vfs
            .write(self.workspace_id, rel, &params.content, self.owner)
            .await
        {
            Ok(()) => json!({"success": true, "path": params.path}),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}
