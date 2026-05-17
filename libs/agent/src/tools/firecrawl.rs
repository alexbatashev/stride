use crate::{AgentConfig, Tool, ToolDesc};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use http_body_util::Full;
use hyper::Request;
use llm::{Function, Tool as LlmTool};
use serde::Deserialize;
use serde_json::error::Category;
use serde_json::{Value, json};
use std::sync::Arc;

const SCRAPE_TIMEOUT_SECS: u64 = 45;
const MAX_MARKDOWN_CHARS: usize = 24_000;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

pub struct FirecrawlTool {
    pub api_key: String,
    pub api_url: String,
}

#[derive(ToolDesc)]
struct FirecrawlParams {
    /// URL of the web page to fetch.
    url: String,
}

#[derive(Deserialize)]
struct ScrapeData {
    markdown: Option<String>,
}

#[derive(Deserialize)]
struct ScrapeResponse {
    success: bool,
    data: Option<ScrapeData>,
    error: Option<String>,
}

#[async_trait(?Send)]
impl Tool for FirecrawlTool {
    fn name(&self) -> &str {
        "firecrawl"
    }

    fn readable_name(&self) -> &str {
        "Firecrawl"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Fetch a web page and return its content as markdown.".to_string(),
                name: self.name().to_owned(),
                parameters: Some(FirecrawlParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match FirecrawlParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };

        match scrape(&params.url, &self.api_url, &self.api_key).await {
            Ok(content) => {
                let (content, truncated) = limit_markdown(content);
                json!({"success": true, "content": content, "truncated": truncated})
            }
            Err(e) => json!({"success": false, "error": e}),
        }
    }
}

async fn scrape(url: &str, api_url: &str, api_key: &str) -> Result<String, String> {
    let body = serde_json::to_string(&json!({
        "url": url,
        "formats": ["markdown"],
        "timeout": SCRAPE_TIMEOUT_SECS * 1000,
    }))
    .map_err(|e| e.to_string())?;

    let endpoint = format!("{}/v1/scrape", api_url.trim_end_matches('/'));
    let req = Request::builder()
        .method("POST")
        .uri(&endpoint)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| e.to_string())?;

    let mut stream = tinynet::stream_request(req).await;
    let mut body = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        body.extend_from_slice(&chunk);

        if body.len() > MAX_RESPONSE_BYTES {
            return Err(format!("response exceeded {MAX_RESPONSE_BYTES} bytes"));
        }

        match serde_json::from_slice::<ScrapeResponse>(&body) {
            Ok(resp) => return scrape_content(resp),
            Err(err) if err.classify() == Category::Eof => {}
            Err(err) => return Err(err.to_string()),
        }
    }

    let resp: ScrapeResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    scrape_content(resp)
}

fn scrape_content(resp: ScrapeResponse) -> Result<String, String> {
    if !resp.success {
        return Err(resp.error.unwrap_or_else(|| "unknown error".to_string()));
    }

    resp.data
        .and_then(|d| d.markdown)
        .ok_or_else(|| "no markdown in response".to_string())
}

fn limit_markdown(content: String) -> (String, bool) {
    if content.chars().count() <= MAX_MARKDOWN_CHARS {
        return (content, false);
    }

    let mut truncated = content.chars().take(MAX_MARKDOWN_CHARS).collect::<String>();
    truncated.push_str("\n\n[Firecrawl output truncated]");
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_markdown_keeps_short_content() {
        let (content, truncated) = limit_markdown("hello".to_string());

        assert_eq!(content, "hello");
        assert!(!truncated);
    }

    #[test]
    fn limit_markdown_truncates_on_char_boundary() {
        let input = "é".repeat(MAX_MARKDOWN_CHARS + 1);
        let (content, truncated) = limit_markdown(input);

        assert!(truncated);
        assert!(content.ends_with("[Firecrawl output truncated]"));
        assert_eq!(content.matches('é').count(), MAX_MARKDOWN_CHARS);
    }
}
