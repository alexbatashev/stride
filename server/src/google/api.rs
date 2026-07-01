//! Gmail, Calendar, and Drive operations layered on [`super::GoogleService`].
//! Every call acquires a fresh access token, so token refresh is transparent to
//! callers. Gmail is read plus draft only.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::time::timeout;
use uuid::Uuid;

use super::{GoogleService, REQUEST_TIMEOUT, mime, percent_encode};

const GMAIL_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const CALENDAR_BASE: &str = "https://www.googleapis.com/calendar/v3/calendars/primary";
const DRIVE_BASE: &str = "https://www.googleapis.com/drive/v3/files";

/// Newest-first cap on how many Gmail messages a single list/poll returns.
const GMAIL_FETCH_LIMIT: usize = 25;
/// Characters of a Gmail body or Drive file kept; longer content is truncated.
const MAX_BODY_CHARS: usize = 20_000;

/// A Gmail message in the shape the agent and the trigger payload consume.
#[derive(Clone, Debug, Serialize)]
pub struct GmailMessage {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub body: String,
    /// Milliseconds since the unix epoch; the trigger watermark.
    pub internal_date: i64,
}

/// New inbox messages past a watermark, plus the advanced watermark.
#[derive(Clone, Debug)]
pub struct NewGmailBatch {
    pub messages: Vec<GmailMessage>,
    pub cursor: i64,
}

/// Fields accepted when creating a calendar event. Times are RFC 3339 for timed
/// events or `YYYY-MM-DD` for all-day events.
#[derive(Clone, Debug)]
pub struct CalendarEventInput {
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub attendees: Vec<String>,
}

impl GoogleService {
    // ---- Gmail -------------------------------------------------------------

    /// List the newest inbox messages.
    pub async fn gmail_list_inbox(
        &self,
        user: Uuid,
        limit: usize,
    ) -> Result<Vec<GmailMessage>, String> {
        let limit = limit.clamp(1, GMAIL_FETCH_LIMIT);
        let url = format!("{GMAIL_BASE}/messages?labelIds=INBOX&maxResults={limit}");
        let list = self.api_get(user, &url).await?;
        let ids = message_ids(&list);
        let mut messages = Vec::new();
        for id in ids.into_iter().take(limit) {
            if let Ok(message) = self.gmail_get(user, &id).await {
                messages.push(message);
            }
        }
        messages.sort_by_key(|message| std::cmp::Reverse(message.internal_date));
        Ok(messages)
    }

    /// Fetch and parse a single Gmail message.
    pub async fn gmail_get(&self, user: Uuid, id: &str) -> Result<GmailMessage, String> {
        let url = format!("{GMAIL_BASE}/messages/{}?format=full", percent_encode(id));
        let value = self.api_get(user, &url).await?;
        Ok(parse_gmail_message(&value))
    }

    /// Inbox messages newer than `after` (ms since epoch), with the watermark
    /// advanced to the newest seen.
    pub async fn gmail_new_since(&self, user: Uuid, after: i64) -> Result<NewGmailBatch, String> {
        let messages = self.gmail_list_inbox(user, GMAIL_FETCH_LIMIT).await?;
        let fresh: Vec<GmailMessage> = messages
            .into_iter()
            .filter(|message| message.internal_date > after)
            .collect();
        let cursor = fresh
            .iter()
            .map(|message| message.internal_date)
            .max()
            .unwrap_or(after);
        Ok(NewGmailBatch {
            messages: fresh,
            cursor,
        })
    }

    /// The newest inbox message's `internalDate`, used to baseline a trigger so
    /// existing mail does not fire it. Zero when the inbox is empty.
    pub async fn gmail_latest_internal_date(&self, user: Uuid) -> Result<i64, String> {
        let messages = self.gmail_list_inbox(user, 1).await?;
        Ok(messages.first().map(|m| m.internal_date).unwrap_or(0))
    }

    /// Save a reply-all draft to an existing message. Recipients and threading
    /// headers come from the original; this never sends mail.
    pub async fn gmail_draft_reply(
        &self,
        user: Uuid,
        message_id: &str,
        body: &str,
    ) -> Result<Value, String> {
        let self_email = self.linked_email(user).await.unwrap_or_default();
        let original = self.gmail_get(user, message_id).await?;
        let headers = self.gmail_reply_headers(user, message_id).await?;
        let raw = mime::build_reply(&self_email, &original, &headers, body);
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());
        let payload = json!({
            "message": { "threadId": original.thread_id, "raw": encoded }
        });
        let url = format!("{GMAIL_BASE}/drafts");
        let value = self.api_send(user, "POST", &url, payload).await?;
        Ok(json!({
            "success": true,
            "sent": false,
            "draft_id": value.get("id").and_then(Value::as_str).unwrap_or_default(),
            "to": headers.reply_to,
            "subject": mime::reply_subject(&original.subject),
        }))
    }

    /// Fetch the In-Reply-To/References/Message-ID/From headers needed to thread
    /// a reply.
    async fn gmail_reply_headers(
        &self,
        user: Uuid,
        message_id: &str,
    ) -> Result<mime::ReplyHeaders, String> {
        let url = format!(
            "{GMAIL_BASE}/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Reply-To&metadataHeaders=Message-ID&metadataHeaders=References",
            percent_encode(message_id)
        );
        let value = self.api_get(user, &url).await?;
        Ok(mime::reply_headers(&value))
    }

    // ---- Calendar ----------------------------------------------------------

    /// List upcoming primary-calendar events from `time_min` (RFC 3339).
    pub async fn calendar_list(
        &self,
        user: Uuid,
        time_min: &str,
        max_results: usize,
    ) -> Result<Value, String> {
        let url = format!(
            "{CALENDAR_BASE}/events?singleEvents=true&orderBy=startTime&maxResults={}&timeMin={}",
            max_results.clamp(1, 100),
            percent_encode(time_min)
        );
        let value = self.api_get(user, &url).await?;
        let events: Vec<Value> = value
            .get("items")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(summarize_event).collect())
            .unwrap_or_default();
        Ok(json!({ "events": events }))
    }

    /// Insert an event on the primary calendar.
    pub async fn calendar_insert(
        &self,
        user: Uuid,
        event: &CalendarEventInput,
    ) -> Result<Value, String> {
        let (start, end) = if event.all_day {
            (json!({ "date": event.start }), json!({ "date": event.end }))
        } else {
            (
                json!({ "dateTime": event.start }),
                json!({ "dateTime": event.end }),
            )
        };
        let mut body = json!({
            "summary": event.summary,
            "start": start,
            "end": end,
        });
        if let Some(description) = &event.description {
            body["description"] = json!(description);
        }
        if let Some(location) = &event.location {
            body["location"] = json!(location);
        }
        if !event.attendees.is_empty() {
            body["attendees"] = Value::Array(
                event
                    .attendees
                    .iter()
                    .map(|email| json!({ "email": email }))
                    .collect(),
            );
        }
        let url = format!("{CALENDAR_BASE}/events");
        let value = self.api_send(user, "POST", &url, body).await?;
        Ok(json!({
            "success": true,
            "event_id": value.get("id").and_then(Value::as_str).unwrap_or_default(),
            "html_link": value.get("htmlLink").and_then(Value::as_str).unwrap_or_default(),
        }))
    }

    // ---- Drive -------------------------------------------------------------

    /// List Drive files, optionally filtered by a name substring `query`.
    pub async fn drive_list(
        &self,
        user: Uuid,
        query: Option<&str>,
        max_results: usize,
    ) -> Result<Value, String> {
        let mut url = format!(
            "{DRIVE_BASE}?pageSize={}&orderBy=modifiedTime desc&fields={}",
            max_results.clamp(1, 100),
            percent_encode("files(id,name,mimeType,modifiedTime,size,webViewLink),nextPageToken")
        );
        let trimmed = query.map(str::trim).filter(|q| !q.is_empty());
        if let Some(query) = trimmed {
            let escaped = query.replace('\'', "\\'");
            url.push_str(&format!(
                "&q={}",
                percent_encode(&format!("name contains '{escaped}' and trashed = false"))
            ));
        } else {
            url.push_str(&format!("&q={}", percent_encode("trashed = false")));
        }
        let value = self.api_get(user, &url).await?;
        Ok(json!({ "files": value.get("files").cloned().unwrap_or(json!([])) }))
    }

    /// Fetch a Drive file's text content. Google-native documents are exported as
    /// plain text; other files are downloaded and returned when they decode as
    /// UTF-8. Binary files return metadata with a note.
    pub async fn drive_fetch(&self, user: Uuid, file_id: &str) -> Result<Value, String> {
        let meta_url = format!(
            "{DRIVE_BASE}/{}?fields={}",
            percent_encode(file_id),
            percent_encode("id,name,mimeType,size,modifiedTime,webViewLink")
        );
        let meta = self.api_get(user, &meta_url).await?;
        let mime_type = meta
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let download_url = if mime_type.starts_with("application/vnd.google-apps") {
            let export = export_mime(&mime_type);
            if export.is_empty() {
                return Ok(json!({
                    "success": true,
                    "file": meta,
                    "content": Value::Null,
                    "note": format!("{mime_type} cannot be exported as text"),
                }));
            }
            format!(
                "{DRIVE_BASE}/{}/export?mimeType={}",
                percent_encode(file_id),
                percent_encode(export)
            )
        } else {
            format!("{DRIVE_BASE}/{}?alt=media", percent_encode(file_id))
        };

        let (status, bytes) = self.api_get_bytes(user, &download_url).await?;
        if !(200..300).contains(&status) {
            return Err(format!("Drive download returned status {status}"));
        }
        let (content, note) = match String::from_utf8(bytes.to_vec()) {
            Ok(text) => {
                let truncated: String = text.chars().take(MAX_BODY_CHARS).collect();
                (Value::String(truncated), Value::Null)
            }
            Err(_) => (
                Value::Null,
                json!("file is binary and was not decoded as text"),
            ),
        };
        Ok(json!({ "success": true, "file": meta, "content": content, "note": note }))
    }

    // ---- HTTP plumbing -----------------------------------------------------

    async fn api_get(&self, user: Uuid, url: &str) -> Result<Value, String> {
        let (status, bytes) = self.request(user, "GET", url, None).await?;
        json_or_error(status, &bytes)
    }

    async fn api_send(
        &self,
        user: Uuid,
        method: &str,
        url: &str,
        body: Value,
    ) -> Result<Value, String> {
        let bytes = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
        let (status, bytes) = self.request(user, method, url, Some(bytes)).await?;
        json_or_error(status, &bytes)
    }

    async fn api_get_bytes(&self, user: Uuid, url: &str) -> Result<(u16, Bytes), String> {
        self.request(user, "GET", url, None).await
    }

    async fn request(
        &self,
        user: Uuid,
        method: &str,
        url: &str,
        body: Option<Vec<u8>>,
    ) -> Result<(u16, Bytes), String> {
        let token = self.access_token(user).await?;
        let has_body = body.is_some();
        let mut builder = Request::builder()
            .method(method)
            .uri(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json");
        if has_body {
            builder = builder.header("Content-Type", "application/json");
        }
        let req = builder
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(|error| error.to_string())?;
        timeout(REQUEST_TIMEOUT, tinynet::send_request(req))
            .await
            .map_err(|_| "Google API request timed out".to_string())?
            .map_err(|error| error.to_string())
    }
}

fn json_or_error(status: u16, bytes: &Bytes) -> Result<Value, String> {
    let value: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?
    };
    if (200..300).contains(&status) {
        return Ok(value);
    }
    let detail = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    Err(format!("Google API returned status {status}: {detail}"))
}

fn message_ids(list: &Value) -> Vec<String> {
    list.get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| message.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_gmail_message(value: &Value) -> GmailMessage {
    let payload = value.get("payload");
    let header = |name: &str| header_value(payload, name).unwrap_or_default();
    let internal_date = value
        .get("internalDate")
        .and_then(Value::as_str)
        .and_then(|date| date.parse::<i64>().ok())
        .unwrap_or(0);
    let body: String = payload
        .and_then(extract_body)
        .map(|body| body.chars().take(MAX_BODY_CHARS).collect())
        .unwrap_or_default();
    GmailMessage {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        thread_id: value
            .get("threadId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        from: header("From"),
        to: header("To"),
        subject: header("Subject"),
        date: header("Date"),
        snippet: value
            .get("snippet")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        body,
        internal_date,
    }
}

fn header_value(payload: Option<&Value>, name: &str) -> Option<String> {
    payload?
        .get("headers")?
        .as_array()?
        .iter()
        .find(|header| {
            header
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|header| header.get("value"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Walk a Gmail payload tree and return the first decoded `text/plain` part,
/// falling back to the top-level body.
fn extract_body(payload: &Value) -> Option<String> {
    let mime_type = payload
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if mime_type == "text/plain"
        && let Some(text) = decode_part_body(payload)
    {
        return Some(text);
    }
    if let Some(parts) = payload.get("parts").and_then(Value::as_array) {
        for part in parts {
            if let Some(text) = extract_body(part) {
                return Some(text);
            }
        }
    }
    if mime_type.is_empty() || mime_type.starts_with("text/") {
        return decode_part_body(payload);
    }
    None
}

fn decode_part_body(part: &Value) -> Option<String> {
    let data = part.get("body")?.get("data")?.as_str()?;
    let bytes = URL_SAFE_NO_PAD.decode(data).ok()?;
    String::from_utf8(bytes).ok()
}

fn summarize_event(event: &Value) -> Value {
    let when = |key: &str| {
        event
            .get(key)
            .map(|value| {
                value
                    .get("dateTime")
                    .or_else(|| value.get("date"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
            .unwrap_or_default()
    };
    json!({
        "id": event.get("id").and_then(Value::as_str).unwrap_or_default(),
        "summary": event.get("summary").and_then(Value::as_str).unwrap_or_default(),
        "location": event.get("location").and_then(Value::as_str).unwrap_or_default(),
        "start": when("start"),
        "end": when("end"),
        "html_link": event.get("htmlLink").and_then(Value::as_str).unwrap_or_default(),
    })
}

/// Export MIME type for a Google-native document, or empty when it has no text
/// representation.
fn export_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "application/vnd.google-apps.document" => "text/plain",
        "application/vnd.google-apps.spreadsheet" => "text/csv",
        "application/vnd.google-apps.presentation" => "text/plain",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_and_internal_date() {
        let value = json!({
            "id": "m1",
            "threadId": "t1",
            "snippet": "hi",
            "internalDate": "1700000000000",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    {"name": "From", "value": "a@example.com"},
                    {"name": "Subject", "value": "Hello"}
                ],
                "body": {"data": "SGVsbG8gd29ybGQ"}
            }
        });
        let message = parse_gmail_message(&value);
        assert_eq!(message.from, "a@example.com");
        assert_eq!(message.subject, "Hello");
        assert_eq!(message.internal_date, 1_700_000_000_000);
        assert_eq!(message.body, "Hello world");
    }

    #[test]
    fn extracts_nested_text_part() {
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {"mimeType": "text/html", "body": {"data": "PGI+"}},
                {"mimeType": "text/plain", "body": {"data": "cGxhaW4"}}
            ]
        });
        assert_eq!(extract_body(&payload).as_deref(), Some("plain"));
    }

    #[test]
    fn message_ids_reads_list() {
        let list = json!({"messages": [{"id": "a"}, {"id": "b"}]});
        assert_eq!(message_ids(&list), vec!["a".to_string(), "b".to_string()]);
    }
}
