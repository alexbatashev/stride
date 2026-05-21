use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use serde::Deserialize;

use super::web_search::{SearchProvider, SearchResult};

#[derive(Deserialize)]
struct ArxivPaper {
    title: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    id: Option<String>,
}

#[derive(Deserialize)]
struct ArxivResponse {
    papers: Vec<ArxivPaper>,
}

pub struct ArxivProvider;

#[async_trait(?Send)]
impl SearchProvider for ArxivProvider {
    fn categories(&self) -> &[&str] {
        &["academic"]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let url = format!(
            "https://searchthearxiv.com/search?query={}",
            percent_encode(query),
        );

        let body = get_body(&url).await?;
        let resp: ArxivResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;

        let papers: Vec<(String, String, String)> = resp
            .papers
            .into_iter()
            .take(limit)
            .filter_map(|p| {
                let id = p.id?;
                Some((
                    id,
                    p.title.unwrap_or_default(),
                    p.abstract_text.unwrap_or_default(),
                ))
            })
            .collect();

        let urls =
            futures::future::join_all(papers.iter().map(|(id, _, _)| resolve_url(id.clone())))
                .await;

        Ok(papers
            .into_iter()
            .zip(urls)
            .map(|((_, title, summary), url)| SearchResult {
                title,
                url,
                summary,
            })
            .collect())
    }
}

async fn resolve_url(id: String) -> String {
    let html_url = format!("https://arxiv.org/html/{}", id);
    let req = arxiv_request("HEAD", &html_url)
        .body(Empty::<Bytes>::new())
        .unwrap();
    match tinynet::send_request(req).await {
        Ok((status, _)) if (200..300).contains(&status) => html_url,
        _ => format!("https://arxiv.org/pdf/{}", id),
    }
}

async fn get_body(url: &str) -> Result<Bytes, String> {
    let req = arxiv_request("GET", url)
        .header("x-requested-with", "XMLHttpRequest")
        .body(Empty::<Bytes>::new())
        .map_err(|e| e.to_string())?;

    let (status, body) = tinynet::send_request(req)
        .await
        .map_err(|e| e.to_string())?;

    if !(200..300).contains(&status) {
        return Err(format!("HTTP {}", status));
    }

    Ok(body)
}

fn arxiv_request(method: &'static str, url: &str) -> hyper::http::request::Builder {
    Request::builder().method(method).uri(url).header(
        "user-agent",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
    )
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
        assert_eq!(percent_encode("transformer models"), "transformer+models");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    }
}
