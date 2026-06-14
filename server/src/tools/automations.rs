use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::cron::Cron;
use crate::db::{AutomationKind, automations};

pub struct ScheduleAutomationTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

#[derive(ToolDesc)]
struct ScheduleAutomationParams {
    /// Short human-readable name for the automation.
    name: String,
    /// Standard 5-field cron schedule, e.g. "0 9 * * 1" for 09:00 every Monday (UTC).
    schedule: String,
    /// Either "agent" to run a prompt as a headless agent, or "python" to run a script.
    kind: String,
    /// The agent prompt or python script to run on schedule.
    task: String,
}

#[async_trait(?Send)]
impl Tool for ScheduleAutomationTool {
    fn name(&self) -> &str {
        "schedule_automation"
    }

    fn readable_name(&self) -> &str {
        "Schedule automation"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Create a recurring scheduled task (automation). It runs on a cron \
                    schedule either as a headless agent prompt or a python script, and the user \
                    can review past executions in the Automations tab."
                    .to_string(),
                parameters: Some(ScheduleAutomationParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match ScheduleAutomationParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };
        let kind = match params.kind.as_str() {
            "python" => AutomationKind::Python,
            "agent" => AutomationKind::Agent,
            other => return json!({"success": false, "error": format!("invalid kind: {other}")}),
        };
        if Cron::parse(params.schedule.trim()).is_err() {
            return json!({"success": false, "error": "invalid cron schedule"});
        }

        let id = Uuid::now_v7();
        let result = automations::insert()
            .id(id)
            .owner(self.user_id)
            .name(params.name.trim())
            .schedule(params.schedule.trim())
            .kind(kind)
            .payload(params.task.as_str())
            .enabled(true)
            .created_at(now())
            .execute(&self.db)
            .await;

        match result {
            Ok(_) => json!({"success": true, "id": id.to_string()}),
            Err(e) => json!({"success": false, "error": e.to_string()}),
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
