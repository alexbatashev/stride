//! The tool an agent calls when it wants to do something the user did not
//! actually ask for. Instead of acting, it parks the proposal in the user's
//! inbox where it can be approved or declined later.

use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde_json::{Value as JsonValue, json};
use stride_agent::{AgentConfig, Tool, ToolDesc};
use uuid::Uuid;

use crate::inbox::{self, Automation, FollowUp, InboxAction, NewSuggestion, Skill};

pub struct SuggestActionTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    pub thread_id: Option<Uuid>,
}

#[derive(ToolDesc)]
struct SuggestActionParams {
    /// What to propose: "follow_up" to explore a related topic in a new thread, "automation" to set up a recurring or triggered task, or "skill" to save reusable know-how.
    kind: String,
    /// Short headline the user sees in their inbox, e.g. "Track competitor pricing weekly".
    title: String,
    /// One or two sentences explaining why you are proposing this and what it would do.
    rationale: String,
    /// For kind "follow_up": the opening message the new thread starts from.
    message: Option<String>,
    /// For kind "follow_up": optional project id or exact title to place the new thread in.
    project: Option<String>,
    /// For kind "automation" or "skill": the name of the automation or skill.
    name: Option<String>,
    /// For kind "automation": standard 5-field cron schedule, e.g. "0 9 * * 1". Required when trigger is "cron".
    schedule: Option<String>,
    /// For kind "automation": "agent" to run a prompt as a headless agent, or "python" to run a script. Defaults to "agent".
    automation_kind: Option<String>,
    /// For kind "automation": the agent prompt or python script to run.
    task: Option<String>,
    /// For kind "automation": what fires it — "cron" (default), "webhook", or "manual".
    trigger: Option<String>,
    /// For kind "automation": where results go besides being stored — "none" (default) or "telegram".
    notify: Option<String>,
    /// For kind "skill": one sentence describing when the skill should be used.
    description: Option<String>,
    /// For kind "skill": the full markdown body of the skill.
    content: Option<String>,
}

fn required(value: Option<String>, field: &str) -> Result<String, String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required for this suggestion kind"))
}

fn build_action(params: SuggestActionParams) -> Result<InboxAction, String> {
    match params.kind.as_str() {
        "follow_up" => Ok(InboxAction::FollowUp(FollowUp {
            message: required(params.message, "message")?,
            project: params
                .project
                .map(|project| project.trim().to_string())
                .filter(|project| !project.is_empty()),
            thread_title: None,
        })),
        "automation" => Ok(InboxAction::Automation(Automation {
            name: required(params.name, "name")?,
            schedule: params.schedule.unwrap_or_default().trim().to_string(),
            kind: params
                .automation_kind
                .unwrap_or_else(|| "agent".to_string())
                .trim()
                .to_string(),
            task: required(params.task, "task")?,
            trigger: params
                .trigger
                .unwrap_or_else(|| "cron".to_string())
                .trim()
                .to_string(),
            notify: params
                .notify
                .unwrap_or_else(|| "none".to_string())
                .trim()
                .to_string(),
        })),
        "skill" => Ok(InboxAction::Skill(Skill {
            name: required(params.name, "name")?,
            description: required(params.description, "description")?,
            content: required(params.content, "content")?,
        })),
        other => Err(format!("invalid kind: {other}")),
    }
}

#[async_trait(?Send)]
impl Tool for SuggestActionTool {
    fn name(&self) -> &str {
        "suggest_action"
    }

    fn readable_name(&self) -> &str {
        "Suggest action"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Propose an action the user did not explicitly ask for and park it \
                    in their Inbox for review. Use this when you notice a worthwhile follow-up \
                    topic, a task worth automating, or reusable know-how worth saving as a skill, \
                    but you are not confident the user wants it right now. The action only runs if \
                    the user approves it, so never use this for work they already asked for — do \
                    that directly instead."
                    .to_string(),
                parameters: Some(SuggestActionParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match SuggestActionParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"success": false, "error": error}),
        };
        let title = params.title.trim().to_string();
        let rationale = params.rationale.trim().to_string();
        if title.is_empty() {
            return json!({"success": false, "error": "title must not be empty"});
        }

        let action = match build_action(params) {
            Ok(action) => action,
            Err(error) => return json!({"success": false, "error": error}),
        };

        let suggestion = NewSuggestion {
            owner: self.user_id,
            thread_id: self.thread_id,
            title,
            rationale,
            action,
        };
        match inbox::suggest(
            &self.db,
            config.clock.as_ref(),
            config.id_gen.as_ref(),
            suggestion,
        )
        .await
        {
            Ok(id) => json!({
                "success": true,
                "id": id.to_string(),
                "status": "pending",
                "note": "Waiting in the user's Inbox. Mention it briefly; do not act on it yet.",
            }),
            Err(error) => json!({"success": false, "error": error}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(kind: &str) -> SuggestActionParams {
        SuggestActionParams {
            kind: kind.to_string(),
            title: "Title".to_string(),
            rationale: "Because".to_string(),
            message: None,
            project: None,
            name: None,
            schedule: None,
            automation_kind: None,
            task: None,
            trigger: None,
            notify: None,
            description: None,
            content: None,
        }
    }

    #[test]
    fn follow_up_needs_a_message() {
        assert!(build_action(params("follow_up")).is_err());
        let mut with_message = params("follow_up");
        with_message.message = Some("Compare the two vendors".to_string());
        assert_eq!(
            build_action(with_message).unwrap(),
            InboxAction::FollowUp(FollowUp {
                message: "Compare the two vendors".to_string(),
                project: None,
                thread_title: None,
            })
        );
    }

    #[test]
    fn automation_defaults_to_a_cron_agent_prompt() {
        let mut fields = params("automation");
        fields.name = Some("Weekly digest".to_string());
        fields.task = Some("Summarize the week".to_string());
        fields.schedule = Some("0 9 * * 1".to_string());
        let action = build_action(fields).unwrap();
        assert_eq!(
            action,
            InboxAction::Automation(Automation {
                name: "Weekly digest".to_string(),
                schedule: "0 9 * * 1".to_string(),
                kind: "agent".to_string(),
                task: "Summarize the week".to_string(),
                trigger: "cron".to_string(),
                notify: "none".to_string(),
            })
        );
        assert!(action.validate().is_ok());
    }

    #[test]
    fn unknown_kinds_are_rejected() {
        assert!(build_action(params("teleport")).is_err());
    }
}
