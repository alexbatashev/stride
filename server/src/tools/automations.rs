use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::cron::Cron;
use crate::db::{AutomationKind, NotifyKind, TriggerKind, automations};
use crate::triggers::webhook;

pub struct ScheduleAutomationTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
}

#[derive(ToolDesc)]
struct ScheduleAutomationParams {
    /// Short human-readable name for the automation.
    name: String,
    /// Standard 5-field cron schedule, e.g. "0 9 * * 1" for 09:00 every Monday (UTC). Required when trigger is "cron"; ignored otherwise.
    schedule: String,
    /// Either "agent" to run a prompt as a headless agent, or "python" to run a script.
    kind: String,
    /// The agent prompt or python script to run.
    task: String,
    /// What fires the automation: "cron" (default), "webhook" (returns a secret URL to call), or "manual" (run on demand only).
    trigger: Option<String>,
    /// Where to push the result besides storing it: "none" (default) or "telegram".
    notify: Option<String>,
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
        let trigger = match params.trigger.as_deref().unwrap_or("cron") {
            "cron" => TriggerKind::Cron,
            "webhook" => TriggerKind::Webhook,
            "manual" => TriggerKind::Manual,
            "email" => {
                return json!({"error": "email automations must be configured from the Automations page"});
            }
            other => return json!({"success": false, "error": format!("invalid trigger: {other}")}),
        };
        let notify = match params.notify.as_deref().unwrap_or("none") {
            "none" => NotifyKind::None,
            "telegram" => NotifyKind::Telegram,
            other => return json!({"success": false, "error": format!("invalid notify: {other}")}),
        };
        if trigger == TriggerKind::Cron && Cron::parse(params.schedule.trim()).is_err() {
            return json!({"success": false, "error": "invalid cron schedule"});
        }

        let webhook_secret = match trigger {
            TriggerKind::Webhook => Some(webhook::generate_secret()),
            _ => None,
        };
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
            .trigger_kind(Some(trigger.as_str()))
            .webhook_secret(webhook_secret.as_deref())
            .notify_kind(Some(notify.as_str()))
            .execute(&self.db)
            .await;

        match result {
            Ok(_) => match webhook_secret {
                Some(secret) => json!({
                    "success": true,
                    "id": id.to_string(),
                    "webhook_url": format!("/api/automations/{id}/webhook"),
                    "webhook_secret": secret,
                }),
                None => json!({"success": true, "id": id.to_string()}),
            },
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
