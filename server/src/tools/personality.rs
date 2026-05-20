use std::sync::Arc;

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use minisql::{ConnectionPool, Value};
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

pub struct UpdatePersonalityTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

#[derive(ToolDesc)]
struct UpdatePersonalityParams {
    /// New personality description for the user. Should not exceed 500-600 words. Describe communication preferences, interests, expertise areas, tone, or any context that helps the agent interact better with this user.
    personality: String,
}

#[async_trait(?Send)]
impl Tool for UpdatePersonalityTool {
    fn name(&self) -> &str {
        "update_personality"
    }

    fn readable_name(&self) -> &str {
        "Update personality"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Update the user personality profile stored in the database. The agent uses this profile to personalize responses. The personality should not be longer than 500-600 words.".to_string(),
                parameters: Some(UpdatePersonalityParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match UpdatePersonalityParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };

        let result = self
            .db
            .query_with_params(
                "UPDATE users SET personality = ? WHERE id = ?",
                vec![Value::Text(params.personality), Value::Uuid(self.user_id)],
            )
            .await;

        match result {
            Ok(_) => json!({"success": true}),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }
}
