use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use http_body_util::Full;
use hyper::Request;
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::db::{telegram_connections, telegram_message_links};

pub struct SendTelegramMessageTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    pub thread_id: Uuid,
    pub bot_token: String,
}

#[derive(ToolDesc)]
struct SendTelegramMessageParams {
    /// Notification text to send to the connected Telegram chat. Keep it concise and include enough context for the user to understand why Friday is notifying them.
    message: String,
}

#[async_trait(?Send)]
impl Tool for SendTelegramMessageTool {
    fn name(&self) -> &str {
        "send_telegram_message"
    }

    fn readable_name(&self) -> &str {
        "Send Telegram message"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Send a Telegram notification to the user in the connected chat's Common area. Use this only when the user asked to be notified or when sending an external notification is clearly part of the task. Replies to the Telegram message will continue this Friday thread.".to_string(),
                parameters: Some(SendTelegramMessageParams::function_parameters()),
            },
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_prompt(&self, args: &JsonValue) -> String {
        let message = args
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        format!("Send Telegram notification: {message}")
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match SendTelegramMessageParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"success": false, "error": error}),
        };
        let message = params.message.trim();
        if message.is_empty() {
            return json!({"success": false, "error": "message is empty"});
        }

        let chat_id = match connected_chat(&self.db, self.user_id).await {
            Ok(Some(chat_id)) => chat_id,
            Ok(None) => return json!({"success": false, "error": "Telegram is not connected"}),
            Err(error) => return json!({"success": false, "error": error}),
        };

        let Some(sent) = send_message(&self.bot_token, chat_id, message).await else {
            return json!({"success": false, "error": "Telegram sendMessage failed"});
        };

        let result = link_message(
            &self.db,
            self.user_id,
            sent.chat_id,
            sent.message_id,
            self.thread_id,
        )
        .await;
        match result {
            Ok(()) => {
                json!({"success": true, "chat_id": sent.chat_id, "message_id": sent.message_id})
            }
            Err(error) => json!({"success": false, "error": error}),
        }
    }
}

async fn connected_chat(db: &ConnectionPool, user_id: Uuid) -> Result<Option<i64>, String> {
    telegram_connections::select_cols((telegram_connections::chat_id,))
        .where_(telegram_connections::user_id.eq(user_id))
        .all(db)
        .await
        .map_err(|error| error.to_string())
        .map(|rows| rows.into_iter().next().map(|(chat_id,)| chat_id))
}

async fn link_message(
    db: &ConnectionPool,
    user_id: Uuid,
    chat_id: i64,
    message_id: i64,
    thread_id: Uuid,
) -> Result<(), String> {
    let _ = telegram_message_links::delete()
        .where_(
            telegram_message_links::user_id
                .eq(user_id)
                .and(telegram_message_links::chat_id.eq(chat_id))
                .and(telegram_message_links::message_id.eq(message_id)),
        )
        .execute(db)
        .await;

    telegram_message_links::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .chat_id(chat_id)
        .message_id(message_id)
        .thread_id(thread_id)
        .execute(db)
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

async fn send_message(bot_token: &str, chat_id: i64, text: &str) -> Option<TelegramSentMessage> {
    let text: String = text.chars().take(4096).collect();
    let body = serde_json::to_vec(&SendMessageRequest {
        chat_id,
        text: &text,
    })
    .ok()?;
    let uri = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .ok()?;

    let (status, body) = tinynet::send_request(req).await.ok()?;
    if !(200..300).contains(&status) {
        return None;
    }

    serde_json::from_slice::<TelegramApiResponse<TelegramSendMessageResult>>(&body)
        .ok()
        .and_then(|response| response.result)
        .map(|message| TelegramSentMessage {
            chat_id: message.chat.id,
            message_id: message.message_id,
        })
}

struct TelegramSentMessage {
    chat_id: i64,
    message_id: i64,
}

#[derive(Serialize)]
struct SendMessageRequest<'a> {
    chat_id: i64,
    text: &'a str,
}

#[derive(Deserialize)]
struct TelegramApiResponse<T> {
    result: Option<T>,
}

#[derive(Deserialize)]
struct TelegramSendMessageResult {
    message_id: i64,
    chat: TelegramSendMessageChat,
}

#[derive(Deserialize)]
struct TelegramSendMessageChat {
    id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn connected_chat_returns_none_without_connection() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        assert_eq!(connected_chat(&db, Uuid::now_v7()).await.unwrap(), None);
    }
}
