use crate::tools::{Tool, ToolContext, ToolDefinition, ToolResult};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;

/// Tool to read the contents of a file
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "read_file",
            "Read the full contents of a file. Use this to view files in the project.",
        )
        .with_param(
            "path",
            "string",
            "The absolute or relative path to the file to read",
            true,
        )
    }

    async fn execute(&self, args: Value, context: &ToolContext) -> ToolResult {
        let path_str = match args["path"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("Missing required parameter 'path'"),
        };

        let path = resolve_path(context, path_str);

        // Security check: ensure path is within current directory
        if let Err(e) = check_path_security(context, &path) {
            return ToolResult::error(e);
        }

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => ToolResult::success(json!({
                "path": path.to_string_lossy(),
                "content": content
            })),
            Err(e) => ToolResult::error(format!("Failed to read file '{}': {}", path_str, e)),
        }
    }
}

/// Tool to list files in a directory
pub struct ListFilesTool;

#[async_trait]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "list_files",
            "List files and directories at a given path. Use this to explore the project structure.",
        )
        .with_param(
            "path",
            "string",
            "The absolute or relative path to the directory to list (default: current directory)",
            false,
        )
    }

    async fn execute(&self, args: Value, context: &ToolContext) -> ToolResult {
        let path_str = args["path"].as_str().unwrap_or(".");
        let path = resolve_path(context, path_str);

        // Security check
        if let Err(e) = check_path_security(context, &path) {
            return ToolResult::error(e);
        }

        match tokio::fs::read_dir(&path).await {
            Ok(mut entries) => {
                let mut files = Vec::new();
                let mut directories = Vec::new();

                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = match entry.file_type().await {
                        Ok(ft) => ft.is_dir(),
                        Err(_) => false,
                    };

                    if is_dir {
                        directories.push(name);
                    } else {
                        files.push(name);
                    }
                }

                directories.sort();
                files.sort();

                ToolResult::success(json!({
                    "path": path.to_string_lossy(),
                    "directories": directories,
                    "files": files
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to list directory '{}': {}", path_str, e)),
        }
    }
}

/// Tool to edit/create files
pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "edit_file",
            "Edit a file by replacing text. If old_string is empty, creates a new file. \
             Use this to modify existing files or create new ones.",
        )
        .with_param(
            "path",
            "string",
            "The absolute or relative path to the file to edit",
            true,
        )
        .with_param(
            "old_string",
            "string",
            "The text to replace. If empty, creates a new file with new_string content",
            true,
        )
        .with_param(
            "new_string",
            "string",
            "The new text to insert in place of old_string (or the full file content if creating new)",
            true,
        )
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &Value) -> String {
        let path = args["path"].as_str().unwrap_or("unknown");
        let old_str = args["old_string"].as_str().unwrap_or("");

        if old_str.is_empty() {
            format!("Create new file: {}", path)
        } else {
            format!("Edit file: {}", path)
        }
    }

    async fn execute(&self, args: Value, context: &ToolContext) -> ToolResult {
        let path_str = match args["path"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("Missing required parameter 'path'"),
        };

        let old_str = args["old_string"].as_str().unwrap_or("");
        let new_str = match args["new_string"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("Missing required parameter 'new_string'"),
        };

        let path = resolve_path(context, path_str);

        // Security check
        if let Err(e) = check_path_security(context, &path) {
            return ToolResult::error(e);
        }

        // Create new file if old_str is empty
        if old_str.is_empty() {
            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolResult::error(format!(
                        "Failed to create parent directories for '{}': {}",
                        path_str, e
                    ));
                }
            }

            match tokio::fs::write(&path, new_str).await {
                Ok(_) => ToolResult::success(json!({
                    "path": path.to_string_lossy(),
                    "action": "created"
                })),
                Err(e) => ToolResult::error(format!("Failed to create file '{}': {}", path_str, e)),
            }
        } else {
            // Edit existing file
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult::error(format!(
                        "Failed to read file '{}': {}. Note: To create a new file, use empty old_string.",
                        path_str, e
                    ));
                }
            };

            // Replace only the first occurrence
            if !content.contains(old_str) {
                return ToolResult::error(format!(
                    "Could not find the specified text to replace in file '{}'",
                    path_str
                ));
            }

            let new_content = content.replacen(old_str, new_str, 1);

            match tokio::fs::write(&path, new_content).await {
                Ok(_) => ToolResult::success(json!({
                    "path": path.to_string_lossy(),
                    "action": "edited"
                })),
                Err(e) => ToolResult::error(format!("Failed to write file '{}': {}", path_str, e)),
            }
        }
    }
}

/// Tool to execute bash commands
pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "bash",
            "Execute a bash command in the shell. Use this to run tests, build, or inspect the project.",
        )
        .with_param(
            "command",
            "string",
            "The bash command to execute",
            true,
        )
        .with_param(
            "working_dir",
            "string",
            "The working directory for the command (default: current directory)",
            false,
        )
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &Value) -> String {
        let cmd = args["command"].as_str().unwrap_or("unknown");
        format!("Execute command: {}", cmd)
    }

    async fn execute(&self, args: Value, context: &ToolContext) -> ToolResult {
        let command = match args["command"].as_str() {
            Some(s) => s,
            None => return ToolResult::error("Missing required parameter 'command'"),
        };

        let working_dir = args["working_dir"]
            .as_str()
            .map(|path| resolve_path(context, path));

        // Block obviously dangerous commands
        if is_dangerous_command(command) {
            return ToolResult::error(
                "This command appears to be potentially destructive and is blocked for safety",
            );
        }

        let mut cmd_builder = tokio::process::Command::new("bash");
        cmd_builder.arg("-c").arg(command);

        cmd_builder.current_dir(working_dir.unwrap_or_else(|| context.cwd.clone()));

        match cmd_builder.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                ToolResult::success(json!({
                    "exit_code": exit_code,
                    "stdout": stdout,
                    "stderr": stderr,
                    "success": output.status.success()
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to execute command: {}", e)),
        }
    }
}

/// Resolve a path string to an absolute PathBuf
fn resolve_path(context: &ToolContext, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        context.cwd.join(path)
    }
}

/// Check if a path is within allowed boundaries (prevents directory traversal)
fn check_path_security(context: &ToolContext, path: &PathBuf) -> Result<(), String> {
    let current_dir = context.cwd.clone();
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    let canonical_current = current_dir.canonicalize().unwrap_or(current_dir);

    // Allow paths within current directory or its subdirectories
    // Also allow /tmp for temporary operations
    if canonical_path.starts_with(&canonical_current) || canonical_path.starts_with("/tmp") {
        Ok(())
    } else {
        Err(format!(
            "Path '{}' is outside the current project directory. \
             For security, only project files can be accessed.",
            path.display()
        ))
    }
}

/// Check if a command is potentially dangerous
fn is_dangerous_command(command: &str) -> bool {
    let dangerous_patterns = [
        "rm -rf /",
        "rm -rf /*",
        ":(){ :|:& };:", // fork bomb
        "> /dev/sda",
        "dd if=/dev/zero",
        "mkfs.",
        "curl.*|.*sh",
        "wget.*|.*sh",
    ];

    let lower_cmd = command.to_lowercase();
    dangerous_patterns
        .iter()
        .any(|pattern| lower_cmd.contains(&pattern.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path() {
        let context = ToolContext {
            cwd: std::env::current_dir().unwrap(),
        };
        let path = resolve_path(&context, "src/main.rs");
        assert!(path.is_absolute());
    }

    #[test]
    fn test_is_dangerous_command() {
        assert!(is_dangerous_command("rm -rf /"));
        assert!(!is_dangerous_command("echo hello"));
    }
}
