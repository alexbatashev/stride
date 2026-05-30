use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use serde::Deserialize;

use super::{SearchProvider, SearchResult};

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
        tracing::debug!(url = %url, limit, "searxng search request");

        let req = Request::builder()
            .method("GET")
            .uri(&url)
            .body(Empty::<Bytes>::new())
            .map_err(|e| e.to_string())?;

        let (status, body) = tinynet::send_request(req)
            .await
            .map_err(|e| e.to_string())?;
        tracing::debug!(
            status,
            body_bytes = body.len(),
            "searxng search response received"
        );

        if !(200..300).contains(&(status as usize)) {
            return Err(format!("HTTP {}", status));
        }

        let resp: SearxngResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        tracing::debug!(
            result_count = resp.results.len(),
            "searxng search response parsed"
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_spaces_and_special() {
        assert_eq!(percent_encode("hello world"), "hello+world");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    }
}
