//! Minimal RFC 822 builder for Gmail reply drafts. Mirrors the IMAP draft
//! builder: it threads the reply with In-Reply-To/References and never adds any
//! send capability.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::Value;
use uuid::Uuid;

use super::api::GmailMessage;

/// Headers pulled from the original message needed to thread a reply.
pub struct ReplyHeaders {
    /// Address the reply is sent to (the original sender or its Reply-To).
    pub reply_to: String,
    pub message_id: Option<String>,
    pub references: Option<String>,
}

pub fn reply_headers(value: &Value) -> ReplyHeaders {
    let header = |name: &str| header_value(value, name);
    let reply_to = header("Reply-To")
        .or_else(|| header("From"))
        .unwrap_or_default();
    ReplyHeaders {
        reply_to,
        message_id: header("Message-ID"),
        references: header("References"),
    }
}

fn header_value(value: &Value, name: &str) -> Option<String> {
    value
        .get("payload")?
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

/// Build the raw RFC 822 message for a reply draft.
pub fn build_reply(
    self_email: &str,
    original: &GmailMessage,
    headers: &ReplyHeaders,
    body: &str,
) -> String {
    let mut lines = Vec::new();
    if !self_email.is_empty() {
        lines.push(format!("From: {}", safe(self_email)));
    }
    lines.push(format!("To: {}", safe(&headers.reply_to)));
    lines.push(format!(
        "Subject: {}",
        encode_header(&reply_subject(&original.subject))
    ));
    if let Some(message_id) = &headers.message_id {
        let message_id = format_message_id(message_id);
        lines.push(format!("In-Reply-To: {message_id}"));
        let mut references = headers
            .references
            .clone()
            .map(|value| safe(&value))
            .unwrap_or_default();
        if !references.is_empty() {
            references.push(' ');
        }
        references.push_str(&message_id);
        lines.push(format!("References: {references}"));
    }
    lines.push("MIME-Version: 1.0".to_string());
    lines.push("Content-Type: text/plain; charset=UTF-8".to_string());
    lines.push("Content-Transfer-Encoding: 8bit".to_string());
    format!("{}\r\n\r\n{}", lines.join("\r\n"), crlf(body))
}

pub fn reply_subject(subject: &str) -> String {
    let subject = safe(subject).trim().to_string();
    if subject.to_ascii_lowercase().starts_with("re:") {
        subject
    } else {
        format!("Re: {subject}")
    }
}

fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        value.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", BASE64.encode(value.as_bytes()))
    }
}

fn safe(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn format_message_id(value: &str) -> String {
    let value = safe(value);
    let value = value.trim().trim_matches(['<', '>']);
    if value.is_empty() {
        format!("<{}@stride.invalid>", Uuid::now_v7())
    } else {
        format!("<{value}>")
    }
}

fn crlf(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(subject: &str) -> GmailMessage {
        GmailMessage {
            id: "m1".to_string(),
            thread_id: "t1".to_string(),
            from: "sender@example.com".to_string(),
            to: "me@example.com".to_string(),
            subject: subject.to_string(),
            date: String::new(),
            snippet: String::new(),
            body: String::new(),
            internal_date: 0,
        }
    }

    #[test]
    fn reply_threads_and_has_no_send_capability() {
        let headers = ReplyHeaders {
            reply_to: "sender@example.com".to_string(),
            message_id: Some("<original@example.com>".to_string()),
            references: None,
        };
        let raw = build_reply("me@example.com", &message("Hello"), &headers, "Hi there");
        assert!(raw.contains("To: sender@example.com"));
        assert!(raw.contains("Subject: Re: Hello"));
        assert!(raw.contains("In-Reply-To: <original@example.com>"));
        assert!(raw.contains("Hi there"));
        assert!(!raw.to_ascii_lowercase().contains("messages/send"));
    }

    #[test]
    fn reply_subject_is_idempotent() {
        assert_eq!(reply_subject("Re: Hello"), "Re: Hello");
        assert_eq!(reply_subject("Hello"), "Re: Hello");
    }
}
