use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use serde::Deserialize;

use super::{SearchProvider, SearchResult};

pub struct SearxngProvider {
    pub endpoint: String,
    pub request_delay: Duration,
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

        let result = tinynet::send_request(req).await;
        // Rate-limit searxng so upstream engines don't ban the instance.
        sleep(self.request_delay).await;
        let (status, body) = result.map_err(|e| e.to_string())?;
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

// Runtime-agnostic sleep: the agent crate deliberately has no async runtime.
async fn sleep(duration: Duration) {
    if duration.is_zero() {
        return;
    }
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(duration);
        let _ = tx.send(());
    });
    let _ = rx.await;
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
    fn sleep_waits_for_duration() {
        let start = std::time::Instant::now();
        futures::executor::block_on(sleep(Duration::from_millis(50)));
        assert!(start.elapsed() >= Duration::from_millis(50));
    }
}
