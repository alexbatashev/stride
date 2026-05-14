use crate::{AgentConfig, Tool, ToolDesc};
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use llm::{Function, Tool as LlmTool};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct FirecrawlTool;

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

        let api_key = match std::env::var("FIRECRAWL_API_KEY") {
            Ok(k) => k,
            Err(_) => return json!({"success": false, "error": "FIRECRAWL_API_KEY not set"}),
        };

        match scrape(&params.url, &api_key).await {
            Ok(content) => json!({"success": true, "content": content}),
            Err(e) => json!({"success": false, "error": e}),
        }
    }
}

async fn scrape(url: &str, api_key: &str) -> Result<String, String> {
    let body = serde_json::to_vec(&json!({"url": url, "formats": ["markdown"]}))
        .map_err(|e| e.to_string())?;

    let req = Request::builder()
        .method("POST")
        .uri("https://api.firecrawl.dev/v1/scrape")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| e.to_string())?;

    let (status, res_body) = tinynet::send_request(req)
        .await
        .map_err(|e| e.to_string())?;

    if !(200..300).contains(&(status as usize)) {
        return Err(format!("HTTP {}", status));
    }

    let resp: ScrapeResponse = serde_json::from_slice(&res_body).map_err(|e| e.to_string())?;

    if !resp.success {
        return Err(resp.error.unwrap_or_else(|| "unknown error".to_string()));
    }

    resp.data
        .and_then(|d| d.markdown)
        .ok_or_else(|| "no markdown in response".to_string())
}
