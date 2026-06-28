//! Native Google tools offered to the agent when the user has linked an account.
//! Gmail (read + draft), Calendar (list + create), and Drive (list + fetch). All
//! operations go through [`crate::google::GoogleService`], which refreshes the
//! access token transparently.

use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};
use stride_agent::{AgentConfig, BaseAgent, Tool, ToolDesc, ToolRegistry};
use uuid::Uuid;

use crate::google::{CalendarEventInput, GoogleService, tool_error};

/// Register every Google tool on `agent`, bound to `user`. The tools are
/// searchable: the agent discovers them through `search_tools` rather than
/// having them always in context.
pub fn register(agent: &BaseAgent, service: GoogleService, user: Uuid) {
    agent.register_searchable_tool(GmailListTool {
        service: service.clone(),
        user,
    });
    agent.register_searchable_tool(GmailDraftReplyTool {
        service: service.clone(),
        user,
    });
    agent.register_searchable_tool(CalendarListTool {
        service: service.clone(),
        user,
    });
    agent.register_searchable_tool(CalendarAddEventTool {
        service: service.clone(),
        user,
    });
    agent.register_searchable_tool(DriveListTool {
        service: service.clone(),
        user,
    });
    agent.register_searchable_tool(DriveFetchTool { service, user });
}

/// Register the read-only Google tools into a scriptable [`ToolRegistry`] so
/// Python automations can call them. Draft and event-creation tools are left out;
/// those change state and belong to the interactive loop.
pub fn register_scriptable(registry: &mut ToolRegistry, service: GoogleService, user: Uuid) {
    registry.register_searchable(GmailListTool {
        service: service.clone(),
        user,
    });
    registry.register_searchable(CalendarListTool {
        service: service.clone(),
        user,
    });
    registry.register_searchable(DriveListTool {
        service: service.clone(),
        user,
    });
    registry.register_searchable(DriveFetchTool { service, user });
}

pub struct GmailListTool {
    service: GoogleService,
    user: Uuid,
}

#[derive(ToolDesc)]
struct GmailListParams {
    /// Maximum number of newest inbox messages to return. Defaults to 10, capped at 25.
    limit: Option<u32>,
}

#[async_trait(?Send)]
impl Tool for GmailListTool {
    fn name(&self) -> &str {
        "gmail_list_emails"
    }

    fn readable_name(&self) -> &str {
        "List Gmail"
    }

    fn definition(&self) -> LlmTool {
        function_tool(
            self.name(),
            "List the newest messages in the connected Gmail inbox, including sender, subject, and body. Read-only.",
            GmailListParams::function_parameters(),
        )
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match GmailListParams::decode(args) {
            Ok(params) => params,
            Err(error) => return tool_error(error),
        };
        let limit = params.limit.unwrap_or(10).clamp(1, 25) as usize;
        match self.service.gmail_list_inbox(self.user, limit).await {
            Ok(messages) => json!({ "success": true, "messages": messages }),
            Err(error) => tool_error(error),
        }
    }
}

pub struct GmailDraftReplyTool {
    service: GoogleService,
    user: Uuid,
}

#[derive(ToolDesc)]
struct GmailDraftReplyParams {
    /// Gmail message id to reply to, as returned by gmail_list_emails.
    message_id: String,
    /// Plain-text reply body.
    body: String,
}

#[async_trait(?Send)]
impl Tool for GmailDraftReplyTool {
    fn name(&self) -> &str {
        "gmail_draft_reply"
    }

    fn readable_name(&self) -> &str {
        "Draft Gmail Reply"
    }

    fn definition(&self) -> LlmTool {
        function_tool(
            self.name(),
            "Create a reply draft to a Gmail message. The draft is saved to Gmail Drafts and is never sent.",
            GmailDraftReplyParams::function_parameters(),
        )
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match GmailDraftReplyParams::decode(args) {
            Ok(params) => params,
            Err(error) => return tool_error(error),
        };
        if params.body.trim().is_empty() {
            return tool_error("reply body cannot be empty");
        }
        match self
            .service
            .gmail_draft_reply(self.user, &params.message_id, &params.body)
            .await
        {
            Ok(value) => value,
            Err(error) => tool_error(error),
        }
    }
}

pub struct CalendarListTool {
    service: GoogleService,
    user: Uuid,
}

#[derive(ToolDesc)]
struct CalendarListParams {
    /// Earliest event start time as an RFC 3339 timestamp (e.g. 2026-06-28T00:00:00Z). Defaults to now.
    time_min: Option<String>,
    /// Maximum number of events to return. Defaults to 10, capped at 100.
    max_results: Option<u32>,
}

#[async_trait(?Send)]
impl Tool for CalendarListTool {
    fn name(&self) -> &str {
        "calendar_list_events"
    }

    fn readable_name(&self) -> &str {
        "List Calendar Events"
    }

    fn definition(&self) -> LlmTool {
        function_tool(
            self.name(),
            "List upcoming events from the connected Google Calendar (primary calendar). Read-only.",
            CalendarListParams::function_parameters(),
        )
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match CalendarListParams::decode(args) {
            Ok(params) => params,
            Err(error) => return tool_error(error),
        };
        let time_min = params.time_min.unwrap_or_else(now_rfc3339);
        let max_results = params.max_results.unwrap_or(10).clamp(1, 100) as usize;
        match self
            .service
            .calendar_list(self.user, &time_min, max_results)
            .await
        {
            Ok(value) => {
                let mut value = value;
                value["success"] = json!(true);
                value
            }
            Err(error) => tool_error(error),
        }
    }
}

pub struct CalendarAddEventTool {
    service: GoogleService,
    user: Uuid,
}

#[derive(ToolDesc)]
struct CalendarAddEventParams {
    /// Event title.
    summary: String,
    /// Start time. RFC 3339 timestamp for timed events, or YYYY-MM-DD when all_day is true.
    start: String,
    /// End time. RFC 3339 timestamp for timed events, or YYYY-MM-DD when all_day is true.
    end: String,
    /// Whether this is an all-day event. Defaults to false.
    all_day: Option<bool>,
    /// Optional event description.
    description: Option<String>,
    /// Optional event location.
    location: Option<String>,
    /// Optional attendee email addresses to invite.
    attendees: Option<Vec<String>>,
}

#[async_trait(?Send)]
impl Tool for CalendarAddEventTool {
    fn name(&self) -> &str {
        "calendar_add_event"
    }

    fn readable_name(&self) -> &str {
        "Add Calendar Event"
    }

    fn definition(&self) -> LlmTool {
        function_tool(
            self.name(),
            "Create an event on the connected Google Calendar (primary calendar).",
            CalendarAddEventParams::function_parameters(),
        )
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match CalendarAddEventParams::decode(args) {
            Ok(params) => params,
            Err(error) => return tool_error(error),
        };
        if params.summary.trim().is_empty() {
            return tool_error("event summary cannot be empty");
        }
        let event = CalendarEventInput {
            summary: params.summary,
            description: params.description,
            location: params.location,
            start: params.start,
            end: params.end,
            all_day: params.all_day.unwrap_or(false),
            attendees: params.attendees.unwrap_or_default(),
        };
        match self.service.calendar_insert(self.user, &event).await {
            Ok(value) => value,
            Err(error) => tool_error(error),
        }
    }
}

pub struct DriveListTool {
    service: GoogleService,
    user: Uuid,
}

#[derive(ToolDesc)]
struct DriveListParams {
    /// Optional name substring to filter files by. Omit to list the most recently modified files.
    query: Option<String>,
    /// Maximum number of files to return. Defaults to 20, capped at 100.
    max_results: Option<u32>,
}

#[async_trait(?Send)]
impl Tool for DriveListTool {
    fn name(&self) -> &str {
        "drive_list_files"
    }

    fn readable_name(&self) -> &str {
        "List Drive Files"
    }

    fn definition(&self) -> LlmTool {
        function_tool(
            self.name(),
            "List files in the connected Google Drive, optionally filtered by a name substring. Read-only.",
            DriveListParams::function_parameters(),
        )
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match DriveListParams::decode(args) {
            Ok(params) => params,
            Err(error) => return tool_error(error),
        };
        let max_results = params.max_results.unwrap_or(20).clamp(1, 100) as usize;
        match self
            .service
            .drive_list(self.user, params.query.as_deref(), max_results)
            .await
        {
            Ok(value) => {
                let mut value = value;
                value["success"] = json!(true);
                value
            }
            Err(error) => tool_error(error),
        }
    }
}

pub struct DriveFetchTool {
    service: GoogleService,
    user: Uuid,
}

#[derive(ToolDesc)]
struct DriveFetchParams {
    /// Google Drive file id, as returned by drive_list_files.
    file_id: String,
}

#[async_trait(?Send)]
impl Tool for DriveFetchTool {
    fn name(&self) -> &str {
        "drive_fetch_file"
    }

    fn readable_name(&self) -> &str {
        "Fetch Drive File"
    }

    fn definition(&self) -> LlmTool {
        function_tool(
            self.name(),
            "Fetch a Google Drive file's text content. Google Docs/Sheets/Slides are exported as text; other text files are returned as-is. Read-only.",
            DriveFetchParams::function_parameters(),
        )
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match DriveFetchParams::decode(args) {
            Ok(params) => params,
            Err(error) => return tool_error(error),
        };
        match self.service.drive_fetch(self.user, &params.file_id).await {
            Ok(value) => value,
            Err(error) => tool_error(error),
        }
    }
}

fn function_tool(name: &str, description: &str, parameters: llm::FunctionParameters) -> LlmTool {
    LlmTool {
        r#type: llm::ToolType::Function,
        function: Function {
            description: description.to_string(),
            name: name.to_owned(),
            parameters: Some(parameters),
        },
    }
}

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_rfc3339_utc(secs as i64)
}

/// Format a unix timestamp as an RFC 3339 UTC string without pulling in a date
/// crate. Uses Hinnant's civil-from-days algorithm.
fn format_rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_rfc3339_epoch() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }
}
