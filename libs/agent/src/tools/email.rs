use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{AgentConfig, Tool, ToolDesc};

#[derive(Clone, Debug, Serialize)]
pub struct EmailAccount {
    pub id: String,
    pub name: String,
    pub address: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct EmailMessage {
    pub uid: u32,
    pub mailbox: String,
    pub message_id: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub date: Option<String>,
    pub body: String,
    pub seen: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum EmailMailbox {
    Inbox,
    Sent,
}

#[derive(Clone, Debug, Serialize)]
pub struct EmailDraft {
    pub mailbox: String,
    pub in_reply_to_uid: u32,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
}

#[async_trait(?Send)]
pub trait EmailProvider: Send + Sync {
    async fn accounts(&self) -> Result<Vec<EmailAccount>, String>;

    async fn list(
        &self,
        account_id: &str,
        mailbox: EmailMailbox,
        limit: usize,
    ) -> Result<Vec<EmailMessage>, String>;

    async fn draft_reply_all(
        &self,
        account_id: &str,
        mailbox: EmailMailbox,
        message_uid: u32,
        body: &str,
    ) -> Result<EmailDraft, String>;
}

pub struct ListEmailsTool {
    pub provider: Arc<dyn EmailProvider>,
}

#[derive(ToolDesc)]
struct ListEmailsParams {
    /// Configured email account ID. Omit to list the user's configured accounts.
    account_id: Option<String>,
    /// Mailbox to read: "inbox" or "sent". Defaults to "inbox".
    mailbox: Option<String>,
    /// Maximum number of newest messages to return. Defaults to 20 and is capped at 100.
    limit: Option<u32>,
}

#[async_trait(?Send)]
impl Tool for ListEmailsTool {
    fn name(&self) -> &str {
        "list_emails"
    }

    fn readable_name(&self) -> &str {
        "List Emails"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "List configured email accounts or fetch the newest incoming or sent emails from one account. This tool is read-only."
                    .to_string(),
                name: self.name().to_owned(),
                parameters: Some(ListEmailsParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match ListEmailsParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"success": false, "error": error}),
        };
        let Some(account_id) = params.account_id else {
            return match self.provider.accounts().await {
                Ok(accounts) => json!({"success": true, "accounts": accounts}),
                Err(error) => json!({"success": false, "error": error}),
            };
        };

        let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
        match self
            .provider
            .list(
                &account_id,
                match parse_mailbox(params.mailbox.as_deref()) {
                    Ok(mailbox) => mailbox,
                    Err(error) => return json!({"success": false, "error": error}),
                },
                limit,
            )
            .await
        {
            Ok(messages) => json!({"success": true, "messages": messages}),
            Err(error) => json!({"success": false, "error": error}),
        }
    }
}

pub struct CreateEmailDraftTool {
    pub provider: Arc<dyn EmailProvider>,
}

#[derive(ToolDesc)]
struct CreateEmailDraftParams {
    /// Configured email account ID.
    account_id: String,
    /// Mailbox containing the message: "inbox" or "sent". Defaults to "inbox".
    mailbox: Option<String>,
    /// IMAP UID of the message to reply to, as returned by list_emails.
    message_uid: u32,
    /// Plain-text reply body.
    body: String,
}

#[async_trait(?Send)]
impl Tool for CreateEmailDraftTool {
    fn name(&self) -> &str {
        "create_email_draft"
    }

    fn readable_name(&self) -> &str {
        "Create Email Draft"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Create a reply-all draft for an existing email. Recipients and threading headers are derived from the original message. This tool can only save to Drafts and cannot send email."
                    .to_string(),
                name: self.name().to_owned(),
                parameters: Some(CreateEmailDraftParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match CreateEmailDraftParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"success": false, "error": error}),
        };
        if params.body.trim().is_empty() {
            return json!({"success": false, "error": "reply body cannot be empty"});
        }

        match self
            .provider
            .draft_reply_all(
                &params.account_id,
                match parse_mailbox(params.mailbox.as_deref()) {
                    Ok(mailbox) => mailbox,
                    Err(error) => return json!({"success": false, "error": error}),
                },
                params.message_uid,
                &params.body,
            )
            .await
        {
            Ok(draft) => json!({"success": true, "draft": draft, "sent": false}),
            Err(error) => json!({"success": false, "error": error}),
        }
    }
}

fn parse_mailbox(value: Option<&str>) -> Result<EmailMailbox, String> {
    match value.unwrap_or("inbox") {
        "inbox" => Ok(EmailMailbox::Inbox),
        "sent" => Ok(EmailMailbox::Sent),
        _ => Err("mailbox must be 'inbox' or 'sent'".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct MockProvider {
        draft_calls: Mutex<Vec<(String, u32, String)>>,
    }

    #[async_trait(?Send)]
    impl EmailProvider for MockProvider {
        async fn accounts(&self) -> Result<Vec<EmailAccount>, String> {
            Ok(vec![EmailAccount {
                id: "account-1".to_string(),
                name: "Work".to_string(),
                address: "me@example.com".to_string(),
            }])
        }

        async fn list(
            &self,
            _account_id: &str,
            _mailbox: EmailMailbox,
            _limit: usize,
        ) -> Result<Vec<EmailMessage>, String> {
            Ok(Vec::new())
        }

        async fn draft_reply_all(
            &self,
            account_id: &str,
            _mailbox: EmailMailbox,
            message_uid: u32,
            body: &str,
        ) -> Result<EmailDraft, String> {
            self.draft_calls.lock().unwrap().push((
                account_id.to_string(),
                message_uid,
                body.to_string(),
            ));
            Ok(EmailDraft {
                mailbox: "Drafts".to_string(),
                in_reply_to_uid: message_uid,
                to: vec!["sender@example.com".to_string()],
                cc: Vec::new(),
                subject: "Re: Test".to_string(),
            })
        }
    }

    fn config() -> Arc<AgentConfig> {
        Arc::new(AgentConfig {
            model_registry: crate::ModelRegistry::new(),
            max_iterations: 1,
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            ..Default::default()
        })
    }

    #[test]
    fn omitting_account_lists_accounts() {
        let provider = Arc::new(MockProvider {
            draft_calls: Mutex::new(Vec::new()),
        });
        let result =
            futures::executor::block_on(ListEmailsTool { provider }.execute(config(), json!({})));
        assert_eq!(result["accounts"][0]["address"], "me@example.com");
    }

    #[test]
    fn draft_tool_only_accepts_existing_message_reference() {
        let provider = Arc::new(MockProvider {
            draft_calls: Mutex::new(Vec::new()),
        });
        let result = futures::executor::block_on(
            CreateEmailDraftTool {
                provider: provider.clone(),
            }
            .execute(
                config(),
                json!({"account_id": "account-1", "message_uid": 42, "body": "Reply"}),
            ),
        );

        assert_eq!(result["success"], true);
        assert_eq!(result["sent"], false);
        assert_eq!(provider.draft_calls.lock().unwrap().len(), 1);
    }
}
