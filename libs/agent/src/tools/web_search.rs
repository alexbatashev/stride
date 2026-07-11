pub mod arxiv;
pub mod brave;
pub mod pubmed;
pub mod searxng;
pub mod uspto;

use crate::{AgentConfig, Tool, ToolDesc};
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
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

pub trait ResultRanker: Send + Sync {
    fn rank(&self, provider_results: Vec<Vec<SearchResult>>) -> Vec<SearchResult>;
}

pub struct InterleaveRanker;

impl ResultRanker for InterleaveRanker {
    fn rank(&self, provider_results: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
        let max_len = provider_results.iter().map(|v| v.len()).max().unwrap_or(0);
        let mut iters: Vec<_> = provider_results
            .into_iter()
            .map(|v| v.into_iter())
            .collect();
        let mut out = Vec::new();
        for _ in 0..max_len {
            for iter in &mut iters {
                if let Some(item) = iter.next() {
                    out.push(item);
                }
            }
        }
        out
    }
}

pub struct WebSearchTool {
    pub providers: Vec<Box<dyn SearchProvider>>,
    pub ranker: Box<dyn ResultRanker>,
}

#[derive(ToolDesc)]
struct WebSearchParams {
    /// The search query string.
    query: String,
    /// Maximum number of results to return. Defaults to 10.
    limit: Option<u32>,
    /// Provider category filter. One of:
    /// - "generic" - use this category for all-purpose searches
    /// - "academic" - use this category for reviewd research paper search
    /// - "patent" - use this category to perform full-text search in patent bureaus like USPTO
    ///
    /// Defaults to "generic".
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
        let category = params.category.as_deref().unwrap_or("generic");
        tracing::debug!(
            query = %params.query,
            limit,
            category,
            providers = self.providers.len(),
            "web_search request"
        );

        let mut provider_results = Vec::new();
        let mut errors = Vec::new();
        for (provider_index, provider) in self.providers.iter().enumerate() {
            if !provider.categories().contains(&category) {
                tracing::debug!(
                    provider_index,
                    provider_categories = ?provider.categories(),
                    category,
                    "web_search provider skipped by category"
                );
                continue;
            }
            match provider.search(&params.query, limit).await {
                Ok(items) => {
                    tracing::debug!(
                        provider_index,
                        provider_categories = ?provider.categories(),
                        result_count = items.len(),
                        "web_search provider returned results"
                    );
                    provider_results.push(items);
                }
                Err(error) => {
                    tracing::warn!(
                        provider_index,
                        provider_categories = ?provider.categories(),
                        error = %error,
                        "web_search provider failed"
                    );
                    errors.push(error);
                }
            }
        }

        let ranked = self.ranker.rank(provider_results);
        let mut seen_urls = std::collections::HashSet::new();
        let items: Vec<Value> = ranked
            .into_iter()
            .filter(|r| seen_urls.insert(r.url.clone()))
            .take(limit)
            .map(|r| json!({"title": r.title, "url": r.url, "summary": r.summary}))
            .collect();
        tracing::debug!(result_count = items.len(), "web_search response");

        if !items.is_empty() {
            json!({"success": true, "results": items})
        } else {
            json!({"success": false, "errors": errors})
        }
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
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            ..Default::default()
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
            ranker: Box::new(InterleaveRanker),
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
            ranker: Box::new(InterleaveRanker),
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
            ranker: Box::new(InterleaveRanker),
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
            ranker: Box::new(InterleaveRanker),
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
    fn interleave_ranker_zips_providers() {
        let tool = WebSearchTool {
            providers: vec![
                Box::new(MockProvider {
                    results: vec![
                        (
                            "A1".to_string(),
                            "https://example.com/a1".to_string(),
                            String::new(),
                        ),
                        (
                            "A2".to_string(),
                            "https://example.com/a2".to_string(),
                            String::new(),
                        ),
                        (
                            "A3".to_string(),
                            "https://example.com/a3".to_string(),
                            String::new(),
                        ),
                    ],
                    cat: "generic",
                }),
                Box::new(MockProvider {
                    results: vec![
                        (
                            "B1".to_string(),
                            "https://example.com/b1".to_string(),
                            String::new(),
                        ),
                        (
                            "B2".to_string(),
                            "https://example.com/b2".to_string(),
                            String::new(),
                        ),
                    ],
                    cat: "generic",
                }),
            ],
            ranker: Box::new(InterleaveRanker),
        };

        let result = futures::executor::block_on(
            tool.execute(make_config(), json!({"query": "test", "limit": 5})),
        );

        assert_eq!(result["success"], true);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 5);
        assert_eq!(results[0]["title"], "A1");
        assert_eq!(results[1]["title"], "B1");
        assert_eq!(results[2]["title"], "A2");
        assert_eq!(results[3]["title"], "B2");
        assert_eq!(results[4]["title"], "A3");
    }
}
