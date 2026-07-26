pub trait StreamingMessageSanitizer {
    fn push_str(&mut self, chunk: &str);
    fn snapshot(&self) -> String;
    fn finish(&mut self) -> String;
}

pub trait MessageSanitizerFactory {
    fn input(&self) -> Box<dyn StreamingMessageSanitizer>;
    fn output(&self) -> Box<dyn StreamingMessageSanitizer>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PlainTextSanitizerFactory;

impl MessageSanitizerFactory for PlainTextSanitizerFactory {
    fn input(&self) -> Box<dyn StreamingMessageSanitizer> {
        Box::new(PlainTextSanitizer::default())
    }

    fn output(&self) -> Box<dyn StreamingMessageSanitizer> {
        Box::new(PlainTextSanitizer::default())
    }
}

#[derive(Clone, Debug, Default)]
pub struct HtmlFormattingSanitizerFactory {
    public_url: Option<String>,
}

impl HtmlFormattingSanitizerFactory {
    pub fn new(public_url: Option<String>) -> Self {
        Self { public_url }
    }
}

impl MessageSanitizerFactory for HtmlFormattingSanitizerFactory {
    fn input(&self) -> Box<dyn StreamingMessageSanitizer> {
        Box::new(PlainTextSanitizer::default())
    }

    fn output(&self) -> Box<dyn StreamingMessageSanitizer> {
        Box::new(HtmlFormattingSanitizer::new(self.public_url.clone()))
    }
}

#[derive(Debug, Default)]
pub struct PlainTextSanitizer {
    output: String,
}

impl StreamingMessageSanitizer for PlainTextSanitizer {
    fn push_str(&mut self, chunk: &str) {
        escape_text_into(chunk, &mut self.output);
    }

    fn snapshot(&self) -> String {
        self.output.clone()
    }

    fn finish(&mut self) -> String {
        self.output.clone()
    }
}

#[derive(Debug, Default)]
pub struct HtmlFormattingSanitizer {
    output: String,
    pending_tag: String,
    pending_entity: String,
    open_tags: Vec<String>,
    public_url: Option<String>,
}

impl HtmlFormattingSanitizer {
    pub fn new(public_url: Option<String>) -> Self {
        Self {
            public_url: public_url.and_then(normalize_public_url),
            ..Default::default()
        }
    }

    pub fn sanitize_complete(input: &str) -> String {
        let mut sanitizer = Self::default();
        sanitizer.push_str(input);
        sanitizer.finish()
    }

    pub fn sanitize_complete_with_public_url(input: &str, public_url: Option<String>) -> String {
        let mut sanitizer = Self::new(public_url);
        sanitizer.push_str(input);
        sanitizer.finish()
    }

    fn push_char(&mut self, c: char) {
        if !self.pending_entity.is_empty() {
            if c == ';' {
                self.pending_entity.push(c);
                if is_html_character_reference(&self.pending_entity) {
                    self.output.push_str(&self.pending_entity);
                } else {
                    escape_text_into(&self.pending_entity, &mut self.output);
                }
                self.pending_entity.clear();
                return;
            }
            if is_entity_char(c) && self.pending_entity.len() < 32 {
                self.pending_entity.push(c);
                return;
            }

            escape_text_into(&self.pending_entity, &mut self.output);
            self.pending_entity.clear();
            self.push_char(c);
            return;
        }

        if !self.pending_tag.is_empty() {
            self.pending_tag.push(c);
            if self.pending_tag.len() == 2 && !can_start_tag(c) {
                escape_text_into(&self.pending_tag, &mut self.output);
                self.pending_tag.clear();
                return;
            }
            if c == '>' {
                self.process_pending_tag();
            } else if self.pending_tag.len() > 512 {
                escape_text_into(&self.pending_tag, &mut self.output);
                self.pending_tag.clear();
            }
            return;
        }

        match c {
            '<' => self.pending_tag.push(c),
            '&' => self.pending_entity.push(c),
            c => escape_char_into(c, &mut self.output),
        }
    }

    fn process_pending_tag(&mut self) {
        let tag = std::mem::take(&mut self.pending_tag);
        let Some(parsed) = parse_tag(&tag) else {
            return;
        };
        if !is_allowed_tag(&parsed.name) {
            return;
        }
        if parsed.closing {
            self.close_tag(&parsed.name);
            return;
        }
        if is_media_tag(&parsed.name) {
            self.process_media_tag(&parsed);
            return;
        }
        if is_void_tag(&parsed.name) {
            self.output.push('<');
            self.output.push_str(&parsed.name);
            self.output.push('>');
            return;
        }

        self.output.push('<');
        self.output.push_str(&parsed.name);
        if parsed.name == "a"
            && let Some(href) = parsed.href.as_deref().filter(|href| is_safe_href(href))
        {
            self.output.push_str(" href=\"");
            escape_attr_into(href, &mut self.output);
            self.output.push('"');
            self.output.push_str(" rel=\"noopener noreferrer\"");
            self.output.push_str(" target=\"_blank\"");
        }
        self.output.push('>');

        if parsed.self_closing {
            self.output.push_str("</");
            self.output.push_str(&parsed.name);
            self.output.push('>');
        } else {
            self.open_tags.push(parsed.name);
        }
    }

    fn process_media_tag(&mut self, parsed: &ParsedTag) {
        let Some(src) = parsed
            .src
            .as_deref()
            .filter(|src| self.media_src_is_allowed(src))
        else {
            return;
        };

        self.output.push('<');
        self.output.push_str(&parsed.name);
        self.output.push_str(" src=\"");
        escape_attr_into(src, &mut self.output);
        self.output.push('"');

        match parsed.name.as_str() {
            "img" => {
                if let Some(alt) = parsed.alt.as_deref() {
                    self.output.push_str(" alt=\"");
                    escape_attr_into(alt, &mut self.output);
                    self.output.push('"');
                }
            }
            "audio" | "video" => {
                self.output.push_str(" controls");
            }
            "iframe" => {
                self.output
                    .push_str(r#" sandbox="allow-scripts" loading="lazy""#);
            }
            _ => {}
        }

        self.output.push('>');
        if parsed.name != "img" {
            self.open_tags.push(parsed.name.clone());
        }
    }

    fn media_src_is_allowed(&self, src: &str) -> bool {
        let Some(public_url) = self.public_url.as_deref() else {
            return false;
        };
        src_matches_public_url(src, public_url)
    }

    fn close_tag(&mut self, name: &str) {
        let Some(index) = self.open_tags.iter().rposition(|tag| tag == name) else {
            return;
        };
        while self.open_tags.len() > index {
            let tag = self.open_tags.pop().unwrap();
            self.output.push_str("</");
            self.output.push_str(&tag);
            self.output.push('>');
        }
    }

    fn close_open_tags_into(&self, output: &mut String) {
        for tag in self.open_tags.iter().rev() {
            output.push_str("</");
            output.push_str(tag);
            output.push('>');
        }
    }
}

impl StreamingMessageSanitizer for HtmlFormattingSanitizer {
    fn push_str(&mut self, chunk: &str) {
        for c in chunk.chars() {
            self.push_char(c);
        }
    }

    fn snapshot(&self) -> String {
        let mut output = self.output.clone();
        if !self.pending_entity.is_empty() {
            escape_text_into(&self.pending_entity, &mut output);
        }
        self.close_open_tags_into(&mut output);
        output
    }

    fn finish(&mut self) -> String {
        if !self.pending_entity.is_empty() {
            escape_text_into(&self.pending_entity, &mut self.output);
            self.pending_entity.clear();
        }
        if !self.pending_tag.is_empty() {
            escape_text_into(&self.pending_tag, &mut self.output);
            self.pending_tag.clear();
        }
        while let Some(tag) = self.open_tags.pop() {
            self.output.push_str("</");
            self.output.push_str(&tag);
            self.output.push('>');
        }
        self.output.clone()
    }
}

#[derive(Debug)]
struct ParsedTag {
    name: String,
    closing: bool,
    self_closing: bool,
    href: Option<String>,
    src: Option<String>,
    alt: Option<String>,
}

fn parse_tag(tag: &str) -> Option<ParsedTag> {
    let inner = tag.strip_prefix('<')?.strip_suffix('>')?.trim();
    if inner.is_empty() || inner.starts_with('!') || inner.starts_with('?') {
        return None;
    }

    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, inner),
    };
    let mut chars = inner.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }

    let name_end = chars
        .find_map(|(index, c)| (!is_tag_name_char(c)).then_some(index))
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    let rest = inner[name_end..].trim();
    let self_closing = !closing && rest.ends_with('/');
    let href = (!closing && name == "a")
        .then(|| parse_attr_value(rest, "href"))
        .flatten();
    let src = (!closing && is_media_tag(&name))
        .then(|| parse_attr_value(rest, "src"))
        .flatten();
    let alt = (!closing && name == "img")
        .then(|| parse_attr_value(rest, "alt"))
        .flatten();

    Some(ParsedTag {
        name,
        closing,
        self_closing,
        href,
        src,
        alt,
    })
}

fn parse_attr_value(mut input: &str, target: &str) -> Option<String> {
    while !input.is_empty() {
        input = input.trim_start();
        if input.starts_with('/') {
            break;
        }
        let name_len = input
            .char_indices()
            .find_map(|(index, c)| (!is_attr_name_char(c)).then_some(index))
            .unwrap_or(input.len());
        if name_len == 0 {
            break;
        }
        let name = input[..name_len].to_ascii_lowercase();
        input = input[name_len..].trim_start();
        if !input.starts_with('=') {
            continue;
        }
        input = input[1..].trim_start();
        let (value, rest) = read_attr_value(input);
        input = rest;
        if name == target {
            return Some(decode_html_attr_entities(&value));
        }
    }
    None
}

fn read_attr_value(input: &str) -> (String, &str) {
    let Some(first) = input.chars().next() else {
        return (String::new(), "");
    };
    if first == '"' || first == '\'' {
        let value_start = first.len_utf8();
        if let Some(end) = input[value_start..].find(first) {
            let value_end = value_start + end;
            return (
                input[value_start..value_end].to_string(),
                &input[value_end + first.len_utf8()..],
            );
        }
        return (input[value_start..].to_string(), "");
    }

    let value_end = input
        .char_indices()
        .find_map(|(index, c)| (c.is_whitespace() || c == '>').then_some(index))
        .unwrap_or(input.len());
    (input[..value_end].to_string(), &input[value_end..])
}

fn is_allowed_tag(name: &str) -> bool {
    matches!(
        name,
        "a" | "b"
            | "audio"
            | "blockquote"
            | "br"
            | "code"
            | "del"
            | "em"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "i"
            | "iframe"
            | "img"
            | "li"
            | "ol"
            | "p"
            | "pre"
            | "s"
            | "strong"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "u"
            | "ul"
            | "video"
    )
}

fn is_void_tag(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img")
}

fn is_media_tag(name: &str) -> bool {
    matches!(name, "audio" | "iframe" | "img" | "video")
}

fn can_start_tag(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?')
}

fn is_tag_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn is_attr_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')
}

fn is_entity_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '#'
}

fn is_html_character_reference(value: &str) -> bool {
    let Some(body) = value
        .strip_prefix('&')
        .and_then(|value| value.strip_suffix(';'))
    else {
        return false;
    };
    if let Some(number) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
        return !number.is_empty() && number.chars().all(|c| c.is_ascii_hexdigit());
    }
    if let Some(number) = body.strip_prefix('#') {
        return !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    }
    !body.is_empty() && body.chars().all(|c| c.is_ascii_alphanumeric())
}

fn decode_html_attr_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('&') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest.find(';') else {
            output.push_str(rest);
            return output;
        };
        let entity = &rest[..=end];
        if let Some(decoded) = decode_html_character_reference(entity) {
            output.push(decoded);
        } else {
            output.push_str(entity);
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

fn decode_html_character_reference(value: &str) -> Option<char> {
    let body = value.strip_prefix('&')?.strip_suffix(';')?;
    match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        _ => {
            let codepoint =
                if let Some(number) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
                    u32::from_str_radix(number, 16).ok()?
                } else {
                    body.strip_prefix('#')?.parse::<u32>().ok()?
                };
            char::from_u32(codepoint)
        }
    }
}

fn is_safe_href(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty() || href.chars().any(|c| c.is_control()) {
        return false;
    }
    let lower = href.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with('/')
        || lower.starts_with('#')
        || !lower.contains(':')
}

fn normalize_public_url(public_url: String) -> Option<String> {
    let public_url = public_url.trim().trim_end_matches('/').to_string();
    (!public_url.is_empty()).then_some(public_url)
}

fn src_matches_public_url(src: &str, public_url: &str) -> bool {
    let src = src.trim();
    if src.is_empty() || src.chars().any(|c| c.is_control()) {
        return false;
    }

    src == public_url
        || src
            .strip_prefix(public_url)
            .is_some_and(|rest| matches!(rest.as_bytes().first(), Some(b'/' | b'?' | b'#')))
}

fn escape_text_into(input: &str, output: &mut String) {
    for c in input.chars() {
        escape_char_into(c, output);
    }
}

fn escape_char_into(c: char, output: &mut String) {
    match c {
        '&' => output.push_str("&amp;"),
        '<' => output.push_str("&lt;"),
        '>' => output.push_str("&gt;"),
        '"' => output.push_str("&quot;"),
        '\'' => output.push_str("&#39;"),
        c => output.push(c),
    }
}

fn escape_attr_into(input: &str, output: &mut String) {
    for c in input.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            c => output.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HtmlFormattingSanitizer, StreamingMessageSanitizer};

    #[test]
    fn escapes_text_and_strips_disallowed_tags() {
        let html = HtmlFormattingSanitizer::sanitize_complete(
            r#"<script>alert(1)</script><strong onclick="x()">ok & done</strong>"#,
        );
        assert_eq!(html, "alert(1)<strong>ok &amp; done</strong>".to_string());
    }

    #[test]
    fn preserves_character_references_and_is_idempotent() {
        let input = "<p>A &amp; B &mdash; &#39; &#x3C;</p><pre><code>&lt;tag&gt;</code></pre>";
        let once = HtmlFormattingSanitizer::sanitize_complete(input);
        let twice = HtmlFormattingSanitizer::sanitize_complete(&once);
        assert_eq!(once, input);
        assert_eq!(twice, once);
    }

    #[test]
    fn preserves_character_references_split_across_chunks() {
        let mut sanitizer = HtmlFormattingSanitizer::default();
        sanitizer.push_str("A &am");
        assert_eq!(sanitizer.snapshot(), "A &amp;am");
        sanitizer.push_str("p; B");
        assert_eq!(sanitizer.finish(), "A &amp; B");
    }

    #[test]
    fn keeps_safe_links_and_removes_unsafe_links() {
        let html = HtmlFormattingSanitizer::sanitize_complete(
            r#"<a href="javascript:alert(1)">bad</a> <a href="/files?a=1&b=2">good</a>"#,
        );
        assert_eq!(
            html,
            r#"<a>bad</a> <a href="/files?a=1&amp;b=2" rel="noopener noreferrer" target="_blank">good</a>"#
        );
    }

    #[test]
    fn decodes_link_entities_before_validation_and_canonicalizes_attributes() {
        let html = HtmlFormattingSanitizer::sanitize_complete(
            r#"<a href="/files?a=1&amp;b=2">good</a><a href="javascript&#58;alert(1)">bad</a>"#,
        );
        assert_eq!(
            html,
            r#"<a href="/files?a=1&amp;b=2" rel="noopener noreferrer" target="_blank">good</a><a>bad</a>"#
        );
    }

    #[test]
    fn keeps_media_from_public_url() {
        let html = HtmlFormattingSanitizer::sanitize_complete_with_public_url(
            r#"<img src="https://stride.example.com/files/a.png" alt="A & B" onerror="x()"><video src="https://stride.example.com/files/a.mp4"></video><audio src="https://stride.example.com/files/a.mp3"></audio><iframe src="https://stride.example.com/files/a.html"></iframe>"#,
            Some("https://stride.example.com/".to_string()),
        );
        assert_eq!(
            html,
            r#"<img src="https://stride.example.com/files/a.png" alt="A &amp; B"><video src="https://stride.example.com/files/a.mp4" controls></video><audio src="https://stride.example.com/files/a.mp3" controls></audio><iframe src="https://stride.example.com/files/a.html" sandbox="allow-scripts" loading="lazy"></iframe>"#
        );
    }

    #[test]
    fn rejects_media_without_public_url() {
        let html = HtmlFormattingSanitizer::sanitize_complete(
            r#"<img src="https://stride.example.com/files/a.png"><iframe src="https://stride.example.com/files/a.html"></iframe>"#,
        );
        assert_eq!(html, "");
    }

    #[test]
    fn rejects_media_from_other_urls_and_prefix_lookalikes() {
        let html = HtmlFormattingSanitizer::sanitize_complete_with_public_url(
            r#"<img src="https://stride.example.com.evil/files/a.png"><img src="https://evil.example.com/files/a.png"><img src="https://stride.example.com/files/a.png">"#,
            Some("https://stride.example.com".to_string()),
        );
        assert_eq!(
            html,
            r#"<img src="https://stride.example.com/files/a.png">"#
        );
    }

    #[test]
    fn snapshot_speculatively_closes_open_tags() {
        let mut sanitizer = HtmlFormattingSanitizer::default();
        sanitizer.push_str("<h1>Hello");
        assert_eq!(sanitizer.snapshot(), "<h1>Hello</h1>");
        sanitizer.push_str("</h1><p>Body");
        assert_eq!(sanitizer.snapshot(), "<h1>Hello</h1><p>Body</p>");
        assert_eq!(sanitizer.finish(), "<h1>Hello</h1><p>Body</p>");
    }

    #[test]
    fn holds_partial_tags_until_they_finish() {
        let mut sanitizer = HtmlFormattingSanitizer::default();
        sanitizer.push_str("<h");
        assert_eq!(sanitizer.snapshot(), "");
        sanitizer.push_str("1>Title");
        assert_eq!(sanitizer.snapshot(), "<h1>Title</h1>");
    }

    #[test]
    fn incomplete_tags_become_text_on_finish() {
        let mut sanitizer = HtmlFormattingSanitizer::default();
        sanitizer.push_str("value <strong");
        assert_eq!(sanitizer.snapshot(), "value ");
        assert_eq!(sanitizer.finish(), "value &lt;strong");
    }
}
