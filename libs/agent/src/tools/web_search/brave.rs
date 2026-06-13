use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use serde::Deserialize;

use super::{SearchProvider, SearchResult};

pub struct BraveProvider {
    pub api_key: String,
    pub endpoint: String,
}

#[derive(Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Deserialize)]
struct BraveResponse {
    web: Option<BraveWeb>,
}

#[async_trait(?Send)]
impl SearchProvider for BraveProvider {
    fn categories(&self) -> &[&str] {
        &["generic"]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        // Brave caps `count` at 20 results per request.
        let url = format!(
            "{}?q={}&count={}",
            self.endpoint.trim_end_matches('/'),
            percent_encode(query),
            limit.min(20),
        );
        tracing::debug!(url = %url, limit, "brave search request");

        let req = Request::builder()
            .method("GET")
            .uri(&url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &self.api_key)
            .body(Empty::<Bytes>::new())
            .map_err(|e| e.to_string())?;

        let (status, body) = tinynet::send_request(req)
            .await
            .map_err(|e| e.to_string())?;
        tracing::debug!(
            status,
            body_bytes = body.len(),
            "brave search response received"
        );

        if !(200..300).contains(&(status as usize)) {
            return Err(format!("HTTP {}", status));
        }

        let resp: BraveResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let results = resp.web.map(|w| w.results).unwrap_or_default();
        tracing::debug!(result_count = results.len(), "brave search response parsed");

        Ok(results
            .into_iter()
            .take(limit)
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                summary: r.description.unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_spaces_and_special() {
        assert_eq!(percent_encode("hello world"), "hello+world");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn parses_web_results() {
        let body = br#"{"web":{"results":[
            {"title":"Example","url":"https://example.com","description":"An example site"},
            {"title":"No desc","url":"https://example.org"}
        ]}}"#;
        let resp: BraveResponse = serde_json::from_slice(body).unwrap();
        let results = resp.web.unwrap().results;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example");
        assert_eq!(results[0].description.as_deref(), Some("An example site"));
        assert_eq!(results[1].description, None);
    }
}
