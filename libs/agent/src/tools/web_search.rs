use crate::{AgentConfig, Tool, ToolDesc};
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use llm::{Function, Tool as LlmTool};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub summary: String,
}

#[async_trait(?Send)]
pub trait SearchProvider: Send + Sync {
    fn categories(&self) -> &[&str];
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String>;
}

pub struct SearxngProvider {
    pub endpoint: String,
}

#[derive(Deserialize)]
struct SearxngResult {
    title: String,
    url: String,
    content: Option<String>,
}

#[derive(Deserialize)]
struct SearxngResponse {
    results: Vec<SearxngResult>,
}

#[async_trait(?Send)]
impl SearchProvider for SearxngProvider {
    fn categories(&self) -> &[&str] {
        &["generic"]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let url = format!(
            "{}/search?q={}&format=json",
            self.endpoint.trim_end_matches('/'),
            percent_encode(query),
        );

        let req = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Empty::<Bytes>::new())
            .map_err(|e| e.to_string())?;

        let (status, body) = tinynet::send_request(req)
            .await
            .map_err(|e| e.to_string())?;

        if !(200..300).contains(&(status as usize)) {
            return Err(format!("HTTP {}", status));
        }

        let resp: SearxngResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;

        Ok(resp
            .results
            .into_iter()
            .take(limit)
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                summary: r.content.unwrap_or_default(),
            })
            .collect())
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0xf) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

pub struct WebSearchTool {
    pub providers: Vec<Box<dyn SearchProvider>>,
}

#[derive(ToolDesc)]
struct WebSearchParams {
    /// The search query string.
    query: String,
    /// Maximum number of results to return. Defaults to 10.
    limit: Option<u32>,
    /// Provider category filter. One of: "all", "generic", "academic". Defaults to "all".
    category: Option<String>,
}

#[async_trait(?Send)]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn readable_name(&self) -> &str {
        "Web Search"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description:
                    "Search the web and return a list of results with titles, URLs, and summaries."
                        .to_string(),
                name: self.name().to_owned(),
                parameters: Some(WebSearchParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match WebSearchParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({"success": false, "error": e}),
        };

        let limit = params.limit.unwrap_or(10) as usize;
        let category = params.category.as_deref().unwrap_or("all");
        let mut seen_urls = std::collections::HashSet::new();
        let mut results = Vec::new();

        for provider in &self.providers {
            if category != "all" && !provider.categories().contains(&category) {
                continue;
            }
            if let Ok(items) = provider.search(&params.query, limit).await {
                for item in items {
                    if seen_urls.insert(item.url.clone()) {
                        results.push(item);
                    }
                }
            }
        }

        let items: Vec<Value> = results
            .into_iter()
            .take(limit)
            .map(|r| json!({"title": r.title, "url": r.url, "summary": r.summary}))
            .collect();

        json!({"success": true, "results": items})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        results: Vec<(String, String, String)>,
        cat: &'static str,
    }

    #[async_trait(?Send)]
    impl SearchProvider for MockProvider {
        fn categories(&self) -> &[&str] {
            std::slice::from_ref(&self.cat)
        }

        async fn search(&self, _query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
            Ok(self
                .results
                .iter()
                .take(limit)
                .map(|(t, u, s)| SearchResult {
                    title: t.clone(),
                    url: u.clone(),
                    summary: s.clone(),
                })
                .collect())
        }
    }

    fn make_config() -> Arc<AgentConfig> {
        Arc::new(AgentConfig {
            model_registry: crate::ModelRegistry::new(),
            max_iterations: 0,
        })
    }

    #[test]
    fn returns_results_from_provider() {
        let tool = WebSearchTool {
            providers: vec![Box::new(MockProvider {
                results: vec![(
                    "Example".to_string(),
                    "https://example.com".to_string(),
                    "An example site".to_string(),
                )],
                cat: "generic",
            })],
        };

        let result =
            futures::executor::block_on(tool.execute(make_config(), json!({"query": "test"})));

        assert_eq!(result["success"], true);
        assert_eq!(result["results"][0]["title"], "Example");
        assert_eq!(result["results"][0]["url"], "https://example.com");
        assert_eq!(result["results"][0]["summary"], "An example site");
    }

    #[test]
    fn respects_limit() {
        let tool = WebSearchTool {
            providers: vec![Box::new(MockProvider {
                results: (0..20)
                    .map(|i| {
                        (
                            format!("Title {}", i),
                            format!("https://example.com/{}", i),
                            String::new(),
                        )
                    })
                    .collect(),
                cat: "generic",
            })],
        };

        let result = futures::executor::block_on(
            tool.execute(make_config(), json!({"query": "test", "limit": 5})),
        );

        assert_eq!(result["success"], true);
        assert_eq!(result["results"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn deduplicates_across_providers() {
        let tool = WebSearchTool {
            providers: vec![
                Box::new(MockProvider {
                    results: vec![(
                        "A".to_string(),
                        "https://example.com/a".to_string(),
                        String::new(),
                    )],
                    cat: "generic",
                }),
                Box::new(MockProvider {
                    results: vec![
                        (
                            "A dup".to_string(),
                            "https://example.com/a".to_string(),
                            String::new(),
                        ),
                        (
                            "B".to_string(),
                            "https://example.com/b".to_string(),
                            String::new(),
                        ),
                    ],
                    cat: "generic",
                }),
            ],
        };

        let result =
            futures::executor::block_on(tool.execute(make_config(), json!({"query": "test"})));

        assert_eq!(result["success"], true);
        assert_eq!(result["results"].as_array().unwrap().len(), 2);
        assert_eq!(result["results"][0]["url"], "https://example.com/a");
        assert_eq!(result["results"][1]["url"], "https://example.com/b");
    }

    #[test]
    fn filters_by_category() {
        let tool = WebSearchTool {
            providers: vec![
                Box::new(MockProvider {
                    results: vec![(
                        "Generic".to_string(),
                        "https://example.com/g".to_string(),
                        String::new(),
                    )],
                    cat: "generic",
                }),
                Box::new(MockProvider {
                    results: vec![(
                        "Academic".to_string(),
                        "https://example.com/a".to_string(),
                        String::new(),
                    )],
                    cat: "academic",
                }),
            ],
        };

        let result = futures::executor::block_on(tool.execute(
            make_config(),
            json!({"query": "test", "category": "academic"}),
        ));

        assert_eq!(result["success"], true);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Academic");
    }

    #[test]
    fn percent_encode_spaces_and_special() {
        assert_eq!(percent_encode("hello world"), "hello+world");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    }
}
