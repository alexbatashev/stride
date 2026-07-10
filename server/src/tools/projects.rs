//! Project and thread management tools for the cloud agent. They let the agent
//! organize work the same way the web UI does: group threads under projects and
//! spin up fresh threads (optionally inside a project) to delegate a task.

use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde_json::{Value as JsonValue, json};
use stride_agent::{AgentConfig, Tool, ToolDesc};
use uuid::Uuid;

use crate::api::threads::DEFAULT_THREAD_TITLE;
use crate::db::{projects, threads};
use crate::runner::AgentRequest;
use crate::runner::inproc::PoolHandle;
use crate::vfs::Vfs;

/// Longest thread title we derive from an opening message.
const MAX_TITLE_LEN: usize = 128;

pub struct CreateProjectTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    pub vfs: Option<Arc<Vfs>>,
}

#[derive(ToolDesc)]
struct CreateProjectParams {
    /// Human-readable project name.
    title: String,
}

#[async_trait(?Send)]
impl Tool for CreateProjectTool {
    fn name(&self) -> &str {
        "create_project"
    }

    fn readable_name(&self) -> &str {
        "Create project"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Create a new project to group related threads under a shared \
                    folder. Returns the project id."
                    .to_string(),
                parameters: Some(CreateProjectParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match CreateProjectParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };
        let title = params.title.trim().to_string();
        if title.is_empty() {
            return json!({"success": false, "error": "title must not be empty"});
        }

        let id = config.id_gen.new_uuid_v7();
        if let Err(e) = projects::insert()
            .id(id)
            .owner(self.user_id)
            .title(title.as_str())
            .execute(&self.db)
            .await
        {
            return json!({"success": false, "error": e.to_string()});
        }

        // Materialize the project's folder so its threads have a writable home,
        // mirroring the HTTP create-project path. Best-effort, like there.
        if let Some(vfs) = &self.vfs
            && let Err(error) = vfs.ensure_project_dir(self.user_id, &title).await
        {
            tracing::warn!(%error, %id, "failed to create project directory");
        }

        json!({"success": true, "id": id.to_string(), "title": title})
    }
}

pub struct ListProjectsTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

#[derive(ToolDesc)]
struct ListProjectsParams {}

#[async_trait(?Send)]
impl Tool for ListProjectsTool {
    fn name(&self) -> &str {
        "list_projects"
    }

    fn readable_name(&self) -> &str {
        "List projects"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "List the user's projects, each with its id and title.".to_string(),
                parameters: Some(ListProjectsParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, _args: JsonValue) -> JsonValue {
        match projects::select()
            .where_(projects::owner.eq(self.user_id))
            .order_by_desc(projects::id)
            .all(&self.db)
            .await
        {
            Ok(rows) => {
                let projects: Vec<JsonValue> = rows
                    .into_iter()
                    .map(|r| json!({"id": r.id.to_string(), "title": r.title}))
                    .collect();
                json!({"success": true, "projects": projects})
            }
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }
}

pub struct StartThreadTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    pub pool: PoolHandle,
}

#[derive(ToolDesc)]
struct StartThreadParams {
    /// The first message that seeds the new thread; the new thread's agent runs on it immediately.
    message: String,
    /// Optional project to place the thread in, given as its id or exact title. Omit to start an ungrouped thread.
    project: Option<String>,
    /// Optional thread title. Defaults to a short title derived from the message.
    title: Option<String>,
}

#[async_trait(?Send)]
impl Tool for StartThreadTool {
    fn name(&self) -> &str {
        "start_thread"
    }

    fn readable_name(&self) -> &str {
        "Start thread"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Start a new thread, optionally inside a project, seeded with an \
                    opening message. The new thread runs independently on its own agent. Returns \
                    the new thread and run ids."
                    .to_string(),
                parameters: Some(StartThreadParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match StartThreadParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };
        let message = params.message.trim().to_string();
        if message.is_empty() {
            return json!({"success": false, "error": "message must not be empty"});
        }

        let project_id = match params.project.as_deref().map(str::trim) {
            Some(reference) if !reference.is_empty() => match self.resolve_project(reference).await
            {
                Ok(id) => Some(id),
                Err(e) => return json!({"success": false, "error": e}),
            },
            _ => None,
        };

        let title = params
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| derive_title(&message));

        let thread_id = config.id_gen.new_uuid_v7();
        let mut insert = threads::insert()
            .id(thread_id)
            .owner(self.user_id)
            .title(title.as_str());
        if let Some(pid) = project_id {
            insert = insert.project_id(pid);
        }
        if let Err(e) = insert.execute(&self.db).await {
            return json!({"success": false, "error": e.to_string()});
        }

        match self
            .pool
            .send(
                thread_id,
                AgentRequest {
                    content: message,
                    images: Vec::new(),
                    model: None,
                },
            )
            .await
        {
            Ok(run_id) => json!({
                "success": true,
                "thread_id": thread_id.to_string(),
                "run_id": run_id.0.to_string(),
                "project_id": project_id.map(|id| id.to_string()),
            }),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }
}

impl StartThreadTool {
    /// Resolve a project reference (id or exact title) to its id, asserting the
    /// user owns it.
    async fn resolve_project(&self, reference: &str) -> Result<Uuid, String> {
        let rows = projects::select()
            .where_(projects::owner.eq(self.user_id))
            .all(&self.db)
            .await
            .map_err(|e| e.to_string())?;

        if let Ok(id) = Uuid::parse_str(reference)
            && rows.iter().any(|r| r.id == id)
        {
            return Ok(id);
        }

        rows.into_iter()
            .find(|r| r.title == reference)
            .map(|r| r.id)
            .ok_or_else(|| format!("project not found: {reference}"))
    }
}

/// Derive a short thread title from its opening message: the first few words,
/// capped in length, falling back to the default when nothing usable remains.
fn derive_title(message: &str) -> String {
    let title: String = message
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_LEN)
        .collect();
    if title.is_empty() {
        DEFAULT_THREAD_TITLE.to_string()
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_title_takes_leading_words() {
        assert_eq!(
            derive_title("Plan the launch of the new website next quarter please"),
            "Plan the launch of the new website next"
        );
    }

    #[test]
    fn derive_title_falls_back_when_blank() {
        assert_eq!(derive_title("   \n\t "), DEFAULT_THREAD_TITLE);
    }

    #[test]
    fn derive_title_caps_length() {
        let long = "x".repeat(500);
        assert_eq!(derive_title(&long).chars().count(), MAX_TITLE_LEN);
    }
}
