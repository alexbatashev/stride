use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use stride_agent::{AgentConfig, Tool, ToolDesc};
use uuid::Uuid;

use tokio::time::timeout;

use crate::db::{telegram_connections, telegram_message_links, telegram_threads};
use crate::vfs::MountedVfs;

pub struct SendTelegramMessageTool {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    pub thread_id: Uuid,
    pub bot_token: String,
}

#[derive(ToolDesc)]
struct SendTelegramMessageParams {
    /// Notification text to send to the connected Telegram chat. Keep it concise and include enough context for the user to understand why Stride is notifying them.
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
                description: "Send a Telegram notification to the user in the connected chat's Common area. Use this only when the user asked to be notified or when sending an external notification is clearly part of the task. Replies to the Telegram message will continue this Stride thread.".to_string(),
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

/// Sends a workspace file to the user as a native Telegram attachment in the chat this thread is
/// bound to. Used in Telegram-originated threads to deliver files the agent produced; its plain
/// text replies are already streamed back to Telegram automatically.
pub struct SendTelegramFileTool {
    pub db: ConnectionPool,
    pub fs: MountedVfs,
    pub user_id: Uuid,
    pub thread_id: Uuid,
    pub bot_token: String,
}

#[derive(ToolDesc)]
struct SendTelegramFileParams {
    /// Absolute workspace path of the file to send, e.g. "/~workspace/report.pdf".
    path: String,
    /// Optional caption shown beneath the file in Telegram.
    caption: Option<String>,
}

/// Telegram rejects multipart uploads larger than 50 MB.
const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

#[async_trait(?Send)]
impl Tool for SendTelegramFileTool {
    fn name(&self) -> &str {
        "send_telegram_file"
    }

    fn readable_name(&self) -> &str {
        "Send Telegram file"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description:
                    "Send a workspace file to the user as a native Telegram attachment in \
                              this conversation. Use this to deliver any file you produced \
                              (documents, images, archives) so it arrives directly in Telegram, on \
                              top of providing a download link."
                        .to_string(),
                parameters: Some(SendTelegramFileParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match SendTelegramFileParams::decode(args) {
            Ok(params) => params,
            Err(error) => return json!({"success": false, "error": error}),
        };

        let (bytes, mime) = match self.fs.read_bytes(&params.path).await {
            Ok(data) => data,
            Err(error) => return json!({"success": false, "error": error.to_string()}),
        };
        if bytes.len() > MAX_UPLOAD_BYTES {
            return json!({"success": false, "error": "file exceeds Telegram's 50 MB upload limit"});
        }

        let Some((chat_id, topic_id)) = thread_chat(&self.db, self.thread_id).await.ok().flatten()
        else {
            return json!({"success": false, "error": "thread is not connected to a Telegram chat"});
        };

        let file_name = file_name_from_path(&params.path);
        let caption = params
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());
        let Some(sent) = send_document(
            &self.bot_token,
            chat_id,
            topic_id,
            &file_name,
            mime.as_deref(),
            &bytes,
            caption,
        )
        .await
        else {
            return json!({"success": false, "error": "Telegram sendDocument failed"});
        };

        match link_message(
            &self.db,
            self.user_id,
            sent.chat_id,
            sent.message_id,
            self.thread_id,
        )
        .await
        {
            Ok(()) => {
                json!({"success": true, "chat_id": sent.chat_id, "message_id": sent.message_id})
            }
            Err(error) => json!({"success": false, "error": error}),
        }
    }
}

fn file_name_from_path(path: &str) -> String {
    let name: String = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect();
    let name = name.trim();
    if name.is_empty() {
        "file".to_string()
    } else {
        name.to_string()
    }
}

/// Resolves the Telegram chat and (optional) topic a thread is bound to. The stored topic id is
/// encoded: positive for forum topics, negative for direct-message topics, `0` for none.
pub(crate) async fn thread_chat(
    db: &ConnectionPool,
    thread_id: Uuid,
) -> Result<Option<(i64, Option<i64>)>, String> {
    telegram_threads::select_cols((telegram_threads::chat_id, telegram_threads::topic_id))
        .where_(telegram_threads::thread_id.eq(thread_id))
        .all(db)
        .await
        .map_err(|error| error.to_string())
        .map(|rows| {
            rows.into_iter()
                .next()
                .map(|(chat_id, topic_id)| (chat_id, (topic_id != 0).then_some(topic_id)))
        })
}

/// Splits an encoded topic id into Telegram's `message_thread_id` (forum topics, positive) and
/// `direct_messages_topic_id` (direct-message topics, stored negative).
fn topic_fields(topic_id: Option<i64>) -> (Option<i64>, Option<i64>) {
    match topic_id {
        Some(id) if id > 0 => (Some(id), None),
        Some(id) if id < 0 => (None, Some(-id)),
        _ => (None, None),
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_document(
    bot_token: &str,
    chat_id: i64,
    topic_id: Option<i64>,
    file_name: &str,
    mime_type: Option<&str>,
    data: &[u8],
    caption: Option<&str>,
) -> Option<TelegramSentMessage> {
    let (message_thread_id, direct_messages_topic_id) = topic_fields(topic_id);
    let mut fields: Vec<(&str, String)> = vec![("chat_id", chat_id.to_string())];
    if let Some(id) = message_thread_id {
        fields.push(("message_thread_id", id.to_string()));
    }
    if let Some(id) = direct_messages_topic_id {
        fields.push(("direct_messages_topic_id", id.to_string()));
    }
    if let Some(caption) = caption {
        fields.push(("caption", caption.chars().take(1024).collect()));
    }

    let boundary = format!("stride{}", Uuid::now_v7().as_simple());
    let mime = mime_type.unwrap_or("application/octet-stream");
    let body = multipart_body(&boundary, &fields, "document", file_name, mime, data);

    let uri = format!("https://api.telegram.org/bot{bot_token}/sendDocument");
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Full::new(Bytes::from(body)))
        .ok()?;

    let (status, body) = match timeout(Duration::from_secs(60), tinynet::send_request(req)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to send Telegram document");
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, "timed out sending Telegram document");
            return None;
        }
    };
    if !(200..300).contains(&status) {
        tracing::warn!(
            status,
            body = %String::from_utf8_lossy(&body),
            "Telegram sendDocument returned error"
        );
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

/// Encodes form fields and a single binary file part as a `multipart/form-data` body.
fn multipart_body(
    boundary: &str,
    fields: &[(&str, String)],
    file_field: &str,
    file_name: &str,
    mime_type: &str,
    data: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{file_field}\"; filename=\"{file_name}\"\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {mime_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

pub(crate) async fn connected_chat(
    db: &ConnectionPool,
    user_id: Uuid,
) -> Result<Option<i64>, String> {
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

/// Telegram rejects `sendMessage` payloads longer than 4096 characters. Longer text is split into
/// several messages instead of being truncated.
pub(crate) const TELEGRAM_MESSAGE_LIMIT: usize = 4096;

/// Telegram's rich (Markdown) messages allow a much larger body than plain `sendMessage`.
pub(crate) const TELEGRAM_RICH_MESSAGE_LIMIT: usize = 32_768;

/// Splits `text` into chunks of at most `limit` characters, preferring to break on newline, then
/// whitespace, so each message stays readable. A run with no break point is hard-split on a
/// character boundary. Empty chunks are dropped; the result is empty only when `text` has no
/// non-whitespace content.
pub(crate) fn split_message(text: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if limit == 0 || chars.len() <= limit {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if chars.len() - start <= limit {
            chunks.push(chars[start..].iter().collect::<String>());
            break;
        }
        let window_end = start + limit;
        let (split_at, drop_separator) = preferred_break(&chars, start, window_end);
        chunks.push(chars[start..split_at].iter().collect::<String>());
        start = if drop_separator {
            split_at + 1
        } else {
            split_at
        };
    }

    chunks
        .into_iter()
        .map(|chunk| chunk.trim().to_string())
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

/// Picks the index to break a chunk at within `[start, window_end)`. Returns the break index and
/// whether the character at that index is a separator to drop (newline or space).
fn preferred_break(chars: &[char], start: usize, window_end: usize) -> (usize, bool) {
    if let Some(index) = (start..window_end).rev().find(|&i| chars[i] == '\n') {
        return (index, true);
    }
    if let Some(index) = (start..window_end).rev().find(|&i| chars[i] == ' ') {
        return (index, true);
    }
    (window_end, false)
}

pub(crate) async fn send_message(
    bot_token: &str,
    chat_id: i64,
    text: &str,
) -> Option<TelegramSentMessage> {
    let mut last_sent = None;
    for chunk in split_message(text, TELEGRAM_MESSAGE_LIMIT) {
        last_sent = Some(send_message_chunk(bot_token, chat_id, &chunk).await?);
    }
    last_sent
}

async fn send_message_chunk(
    bot_token: &str,
    chat_id: i64,
    text: &str,
) -> Option<TelegramSentMessage> {
    let body = serde_json::to_vec(&SendMessageRequest { chat_id, text }).ok()?;
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

pub(crate) struct TelegramSentMessage {
    pub(crate) chat_id: i64,
    pub(crate) message_id: i64,
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

    #[test]
    fn topic_fields_split_by_sign() {
        assert_eq!(topic_fields(None), (None, None));
        assert_eq!(topic_fields(Some(0)), (None, None));
        assert_eq!(topic_fields(Some(5)), (Some(5), None));
        assert_eq!(topic_fields(Some(-7)), (None, Some(7)));
    }

    #[test]
    fn file_name_from_path_takes_basename() {
        assert_eq!(file_name_from_path("/~workspace/report.pdf"), "report.pdf");
        assert_eq!(file_name_from_path("plain.txt"), "plain.txt");
        assert_eq!(file_name_from_path("/~workspace/"), "file");
    }

    #[test]
    fn split_message_keeps_short_text_intact() {
        assert_eq!(split_message("hello", 4096), vec!["hello".to_string()]);
    }

    #[test]
    fn split_message_breaks_on_newline() {
        let text = "aaa\nbbb\nccc";
        let chunks = split_message(text, 5);
        assert_eq!(
            chunks,
            vec!["aaa".to_string(), "bbb".to_string(), "ccc".to_string()]
        );
        assert!(chunks.iter().all(|c| c.chars().count() <= 5));
    }

    #[test]
    fn split_message_breaks_on_space_when_no_newline() {
        let chunks = split_message("aaaa bbbb cccc", 6);
        assert!(chunks.iter().all(|c| c.chars().count() <= 6));
        assert_eq!(chunks.join(" "), "aaaa bbbb cccc");
    }

    #[test]
    fn split_message_hard_splits_long_token() {
        let text = "a".repeat(25);
        let chunks = split_message(&text, 10);
        assert_eq!(chunks, vec!["aaaaaaaaaa", "aaaaaaaaaa", "aaaaa"]);
    }

    #[test]
    fn split_message_respects_limit_on_multibyte() {
        let text = "😀".repeat(20);
        let chunks = split_message(&text, 8);
        assert!(chunks.iter().all(|c| c.chars().count() <= 8));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn multipart_body_includes_fields_and_file() {
        let fields = vec![("chat_id", "42".to_string())];
        let body = multipart_body("BOUND", &fields, "document", "a.txt", "text/plain", b"hi");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("--BOUND\r\n"));
        assert!(text.contains("name=\"chat_id\"\r\n\r\n42\r\n"));
        assert!(text.contains("name=\"document\"; filename=\"a.txt\""));
        assert!(text.contains("Content-Type: text/plain\r\n\r\nhi\r\n"));
        assert!(text.ends_with("--BOUND--\r\n"));
    }
}
