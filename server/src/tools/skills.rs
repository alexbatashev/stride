use std::sync::Arc;

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use minisql::{ConnectionPool, Value};
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

pub struct SearchSkillsTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

pub struct LoadSkillTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

pub struct CreateSkillTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

#[derive(ToolDesc)]
struct SearchSkillsParams {
    /// Keywords to search for among available skills.
    query: String,
}

#[derive(ToolDesc)]
struct LoadSkillParams {
    /// The unique name (slug) of the skill to load.
    name: String,
}

#[derive(ToolDesc)]
struct CreateSkillParams {
    /// Unique slug identifier for the skill, e.g. "python-debugging".
    name: String,
    /// Short human-readable title, e.g. "Python Debugging Guide".
    title: String,
    /// One or two sentence summary of what this skill covers. Used for search.
    description: String,
    /// Full skill content in Markdown. Instructions, context, steps, or domain knowledge the agent should follow when this skill is active.
    content: String,
}

#[async_trait(?Send)]
impl Tool for SearchSkillsTool {
    fn name(&self) -> &str {
        "search_skills"
    }

    fn readable_name(&self) -> &str {
        "Search Skills"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Search available skills by keyword. Returns matching skill names and descriptions. Use load_skill to read the full content of a skill.".to_string(),
                parameters: Some(SearchSkillsParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match SearchSkillsParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let query = params.query.to_lowercase();

        let result = self
            .db
            .query_with_params(
                "SELECT name, title, description FROM skills WHERE owner IS NULL OR owner = ? ORDER BY name ASC",
                vec![Value::Uuid(self.user_id)],
            )
            .await;

        match result {
            Err(e) => json!({"error": e.to_string()}),
            Ok(rows) => {
                let matches: Vec<JsonValue> = rows
                    .rows()
                    .iter()
                    .filter(|row| {
                        let name = row.get_text("name").unwrap_or_default().to_lowercase();
                        let title = row.get_text("title").unwrap_or_default().to_lowercase();
                        let desc = row
                            .get_text("description")
                            .unwrap_or_default()
                            .to_lowercase();
                        name.contains(&query) || title.contains(&query) || desc.contains(&query)
                    })
                    .map(|row| {
                        json!({
                            "name": row.get_text("name").unwrap_or_default(),
                            "title": row.get_text("title").unwrap_or_default(),
                            "description": row.get_text("description").unwrap_or_default(),
                        })
                    })
                    .collect();

                json!({"found": matches.len(), "skills": matches})
            }
        }
    }
}

#[async_trait(?Send)]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn readable_name(&self) -> &str {
        "Load Skill"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Load the full content of a skill by its name. The content contains instructions or context you should follow for the current task.".to_string(),
                parameters: Some(LoadSkillParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match LoadSkillParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"error": e}),
        };

        let result = self
            .db
            .query_with_params(
                "SELECT title, content FROM skills WHERE name = ? AND (owner IS NULL OR owner = ?) LIMIT 1",
                vec![Value::Text(params.name.clone()), Value::Uuid(self.user_id)],
            )
            .await;

        match result {
            Err(e) => json!({"error": e.to_string()}),
            Ok(rows) => match rows.rows().first() {
                None => json!({"error": format!("skill '{}' not found", params.name)}),
                Some(row) => json!({
                    "title": row.get_text("title").unwrap_or_default(),
                    "content": row.get_text("content").unwrap_or_default(),
                }),
            },
        }
    }
}

#[async_trait(?Send)]
impl Tool for CreateSkillTool {
    fn name(&self) -> &str {
        "create_skill"
    }

    fn readable_name(&self) -> &str {
        "Create Skill"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Create a new skill and store it in the database. Skills are reusable instruction sets you can load in future sessions.".to_string(),
                parameters: Some(CreateSkillParams::function_parameters()),
            },
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &JsonValue) -> String {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        format!("Create skill '{name}': {title}")
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match CreateSkillParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };

        let id = Uuid::now_v7();
        let result = self
            .db
            .query_with_params(
                "INSERT INTO skills (id, name, title, description, content, owner) VALUES (?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(id),
                    Value::Text(params.name.clone()),
                    Value::Text(params.title),
                    Value::Text(params.description),
                    Value::Text(params.content),
                    Value::Uuid(self.user_id),
                ],
            )
            .await;

        match result {
            Ok(_) => json!({"success": true, "name": params.name}),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use friday_agent::AgentConfig;
    use minisql::ConnectionPool;

    async fn setup_db() -> (ConnectionPool, Uuid) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let user_id = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(user_id),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();
        (db, user_id)
    }

    fn dummy_config() -> Arc<AgentConfig> {
        Arc::new(AgentConfig {
            model_registry: friday_agent::ModelRegistry::new(),
            max_iterations: 0,
        })
    }

    #[tokio::test]
    async fn create_skill_stores_and_search_finds_it() {
        let (db, user_id) = setup_db().await;
        let config = dummy_config();

        let create = CreateSkillTool {
            db: db.clone(),
            user_id,
        };
        let result = create
            .execute(
                config.clone(),
                json!({
                    "name": "rust-patterns",
                    "title": "Rust Patterns",
                    "description": "Common Rust idioms and patterns",
                    "content": "# Rust Patterns\n\nUse iterators over loops."
                }),
            )
            .await;

        assert_eq!(result["success"], true);
        assert_eq!(result["name"], "rust-patterns");

        let search = SearchSkillsTool {
            db: db.clone(),
            user_id,
        };
        let result = search
            .execute(config.clone(), json!({"query": "rust"}))
            .await;

        assert_eq!(result["found"], 1);
        assert_eq!(result["skills"][0]["name"], "rust-patterns");
    }

    #[tokio::test]
    async fn load_skill_returns_content() {
        let (db, user_id) = setup_db().await;
        let config = dummy_config();

        let create = CreateSkillTool {
            db: db.clone(),
            user_id,
        };
        create
            .execute(
                config.clone(),
                json!({
                    "name": "my-skill",
                    "title": "My Skill",
                    "description": "A test skill",
                    "content": "Do the thing."
                }),
            )
            .await;

        let load = LoadSkillTool {
            db: db.clone(),
            user_id,
        };
        let result = load
            .execute(config.clone(), json!({"name": "my-skill"}))
            .await;

        assert_eq!(result["title"], "My Skill");
        assert_eq!(result["content"], "Do the thing.");
    }

    #[tokio::test]
    async fn load_skill_returns_error_for_missing() {
        let (db, user_id) = setup_db().await;
        let config = dummy_config();

        let load = LoadSkillTool { db, user_id };
        let result = load.execute(config, json!({"name": "nonexistent"})).await;

        assert!(result["error"].as_str().unwrap().contains("nonexistent"));
    }

    #[tokio::test]
    async fn search_skills_no_results_for_unknown_query() {
        let (db, user_id) = setup_db().await;
        let config = dummy_config();

        let search = SearchSkillsTool { db, user_id };
        let result = search.execute(config, json!({"query": "zzznomatch"})).await;

        assert_eq!(result["found"], 0);
    }

    #[tokio::test]
    async fn user_cannot_load_other_users_skill() {
        let (db, user_id) = setup_db().await;
        let config = dummy_config();

        let other_user = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(other_user),
                Value::Text("bob".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();

        let create = CreateSkillTool {
            db: db.clone(),
            user_id: other_user,
        };
        create
            .execute(
                config.clone(),
                json!({
                    "name": "secret-skill",
                    "title": "Secret",
                    "description": "Bob's private skill",
                    "content": "Secret content."
                }),
            )
            .await;

        let load = LoadSkillTool {
            db: db.clone(),
            user_id,
        };
        let result = load
            .execute(config.clone(), json!({"name": "secret-skill"}))
            .await;

        assert!(result.get("error").is_some());

        let search = SearchSkillsTool {
            db: db.clone(),
            user_id,
        };
        let result = search.execute(config, json!({"query": "secret"})).await;

        assert_eq!(result["found"], 0);
    }
}
