//! Splits an agent message into ordered, typed parts so clients can render
//! prose and interactive artifacts as first-class, independently-streamed
//! elements instead of one re-parsed text blob.
//!
//! Only artifact fences (```` ```html ````) split the stream; every other fence
//! stays inside its surrounding text part and renders as a normal code block.
//! The same function feeds the streaming emitter (incrementally), the REST
//! message view, and the reconnect snapshot, so all three agree.

/// Largest artifact body handed to clients. An oversized artifact is downgraded
/// to a plain code block so a runaway document never reaches the sandbox.
pub const MAX_ARTIFACT_BYTES: usize = 256 * 1024;

const ARTIFACT_LANGS: &[&str] = &["html"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartKind {
    Text,
    Artifact,
}

impl PartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PartKind::Text => "text",
            PartKind::Artifact => "artifact",
        }
    }
}

/// One ordered slice of a message. `closed` gates artifact rendering: an artifact
/// is shown only once its fence closes. For text it is cosmetic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessagePart {
    pub kind: PartKind,
    pub lang: Option<String>,
    pub content: String,
    pub closed: bool,
}

impl MessagePart {
    fn text(content: String, closed: bool) -> Self {
        MessagePart {
            kind: PartKind::Text,
            lang: None,
            content,
            closed,
        }
    }
}

/// A fenced opener `(marker, lang)` for a ``` line, else `None`.
fn fence_open(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let ticks = trimmed.chars().take_while(|c| *c == '`').count();
    if ticks < 3 {
        return None;
    }
    let marker = "`".repeat(ticks);
    let lang = trimmed[ticks..].trim().to_string();
    Some((marker, lang))
}

fn is_fence_close(line: &str, marker: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with(marker) && trimmed[marker.len()..].trim().is_empty()
}

fn is_artifact_lang(lang: &str) -> bool {
    ARTIFACT_LANGS.iter().any(|l| lang.eq_ignore_ascii_case(l))
}

fn push_text(parts: &mut Vec<MessagePart>, buffer: &mut Vec<String>) {
    if buffer.is_empty() {
        return;
    }
    let text = buffer.join("\n");
    buffer.clear();
    // Drop blank-line padding around a part without touching inner indentation.
    let trimmed = text.trim_matches('\n');
    if !trimmed.trim().is_empty() {
        parts.push(MessagePart::text(trimmed.to_string(), true));
    }
}

/// Split `content` into ordered parts. When `finalize` is true the content is
/// complete: an artifact whose fence never closed is downgraded to a code block,
/// and the trailing part is marked closed.
pub fn segment(content: &str, finalize: bool) -> Vec<MessagePart> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut parts: Vec<MessagePart> = Vec::new();
    let mut text_buffer: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let Some((marker, lang)) = fence_open(lines[i]) else {
            text_buffer.push(lines[i].to_string());
            i += 1;
            continue;
        };
        if !is_artifact_lang(&lang) {
            text_buffer.push(lines[i].to_string());
            i += 1;
            continue;
        }

        // Artifact fence: flush preceding prose, then collect the body.
        push_text(&mut parts, &mut text_buffer);
        let mut body: Vec<String> = Vec::new();
        let mut j = i + 1;
        let mut closed = false;
        while j < lines.len() {
            if is_fence_close(lines[j], &marker) {
                closed = true;
                break;
            }
            body.push(lines[j].to_string());
            j += 1;
        }
        let source = body.join("\n");
        let oversized = source.len() > MAX_ARTIFACT_BYTES;
        if oversized || (finalize && !closed) {
            // Render as code rather than an artifact.
            let fence = if oversized { "```text" } else { "```html" };
            let mut block = format!("{fence}\n{source}");
            if closed {
                block.push_str("\n```");
            }
            parts.push(MessagePart::text(block, closed || finalize));
        } else {
            parts.push(MessagePart {
                kind: PartKind::Artifact,
                lang: Some(lang),
                content: source,
                closed,
            });
        }
        i = if closed { j + 1 } else { j };
    }

    push_text(&mut parts, &mut text_buffer);

    // The trailing part is still streaming unless we are finalizing.
    if !finalize
        && let Some(last) = parts.last_mut()
        && last.kind == PartKind::Text
    {
        last.closed = false;
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_one_open_part() {
        let parts = segment("hello world", false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, PartKind::Text);
        assert!(!parts[0].closed);
        assert_eq!(parts[0].content, "hello world");
    }

    #[test]
    fn prose_then_artifact_splits_and_closes_prose() {
        let parts = segment("intro\n\n```html\n<b>hi</b>\n```", true);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].kind, PartKind::Text);
        assert!(parts[0].closed);
        assert_eq!(parts[1].kind, PartKind::Artifact);
        assert_eq!(parts[1].content, "<b>hi</b>");
        assert!(parts[1].closed);
    }

    #[test]
    fn open_artifact_is_not_closed_while_streaming() {
        let parts = segment("```html\n<div>partial", false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, PartKind::Artifact);
        assert!(!parts[0].closed);
    }

    #[test]
    fn unterminated_artifact_downgrades_to_code_on_finalize() {
        let parts = segment("```html\n<div>partial", true);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, PartKind::Text);
        assert!(parts[0].content.starts_with("```html\n<div>partial"));
    }

    #[test]
    fn non_artifact_fence_stays_in_text() {
        let parts = segment("```js\nconst a = 1;\n```", true);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, PartKind::Text);
        assert!(parts[0].content.contains("const a = 1;"));
    }

    #[test]
    fn oversized_artifact_downgrades_to_code() {
        let big = "x".repeat(MAX_ARTIFACT_BYTES + 1);
        let parts = segment(&format!("```html\n{big}\n```"), true);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, PartKind::Text);
        assert!(parts[0].content.starts_with("```text"));
    }

    #[test]
    fn artifact_between_prose() {
        let parts = segment("before\n\n```html\n<i>x</i>\n```\n\nafter", true);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].kind, PartKind::Text);
        assert_eq!(parts[1].kind, PartKind::Artifact);
        assert_eq!(parts[2].kind, PartKind::Text);
        assert_eq!(parts[2].content, "after");
    }
}
