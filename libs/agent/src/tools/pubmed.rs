use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::Request;
use serde::Deserialize;

use super::web_search::{SearchProvider, SearchResult};

#[derive(Deserialize)]
struct EsearchResult {
    idlist: Vec<String>,
}

#[derive(Deserialize)]
struct EsearchResponse {
    esearchresult: EsearchResult,
}

pub struct PubmedProvider;

#[async_trait(?Send)]
impl SearchProvider for PubmedProvider {
    fn categories(&self) -> &[&str] {
        &["academic"]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let search_url = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi?db=pubmed&term={}&retmax={}&retmode=json",
            percent_encode(query),
            limit,
        );

        let req = Request::builder()
            .method("GET")
            .uri(&search_url)
            .body(Empty::<Bytes>::new())
            .map_err(|e| e.to_string())?;

        let (status, body) = tinynet::send_request(req)
            .await
            .map_err(|e| e.to_string())?;

        if !(200..300).contains(&(status as usize)) {
            return Err(format!("HTTP {}", status));
        }

        let resp: EsearchResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let ids = resp.esearchresult.idlist;

        if ids.is_empty() {
            return Ok(vec![]);
        }

        let fetch_url = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/efetch.fcgi?db=pubmed&id={}&retmode=xml",
            ids.join(","),
        );

        let req = Request::builder()
            .method("GET")
            .uri(&fetch_url)
            .body(Empty::<Bytes>::new())
            .map_err(|e| e.to_string())?;

        let (status, body) = tinynet::send_request(req)
            .await
            .map_err(|e| e.to_string())?;

        if !(200..300).contains(&(status as usize)) {
            return Err(format!("HTTP {}", status));
        }

        let xml = std::str::from_utf8(&body).map_err(|e| e.to_string())?;
        Ok(parse_articles(xml))
    }
}

fn parse_articles(xml: &str) -> Vec<SearchResult> {
    xml.split("<PubmedArticle>")
        .skip(1)
        .filter_map(|chunk| {
            let pmid = xml_inner(chunk, "PMID")?;
            let title = xml_inner(chunk, "ArticleTitle")?;
            let abstract_parts = xml_all_inner(chunk, "AbstractText");
            let summary = abstract_parts.join(" ");
            Some(SearchResult {
                title: title.to_string(),
                url: format!("https://pubmed.ncbi.nlm.nih.gov/{}/", pmid),
                summary,
            })
        })
        .collect()
}

// Finds text content of the first occurrence of `tag` (handles attributes).
fn xml_inner<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}", tag);
    let tag_pos = s.find(&open)?;
    let content_start = s[tag_pos..].find('>')? + tag_pos + 1;
    let close = format!("</{}>", tag);
    let content_end = s[content_start..].find(&close)? + content_start;
    Some(&s[content_start..content_end])
}

// Collects text content of all occurrences of `tag`.
fn xml_all_inner<'a>(s: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut results = Vec::new();
    let mut pos = 0;
    while pos < s.len() {
        let Some(rel_start) = s[pos..].find(&open) else {
            break;
        };
        let abs_start = pos + rel_start;
        let Some(content_start) = s[abs_start..].find('>').map(|i| abs_start + i + 1) else {
            break;
        };
        let Some(content_end) = s[content_start..].find(&close).map(|i| content_start + i) else {
            break;
        };
        results.push(&s[content_start..content_end]);
        pos = content_end + close.len();
    }
    results
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
    fn parse_articles_extracts_fields() {
        let xml = r#"<?xml version="1.0"?>
<PubmedArticleSet>
<PubmedArticle>
  <MedlineCitation>
    <PMID Version="1">12345678</PMID>
    <Article>
      <ArticleTitle>Effects of Exercise on Cognition</ArticleTitle>
      <Abstract>
        <AbstractText>Regular exercise improves cognitive function.</AbstractText>
      </Abstract>
    </Article>
  </MedlineCitation>
</PubmedArticle>
<PubmedArticle>
  <MedlineCitation>
    <PMID Version="1">87654321</PMID>
    <Article>
      <ArticleTitle>Sleep and Memory Consolidation</ArticleTitle>
      <Abstract>
        <AbstractText Label="BACKGROUND">Sleep is critical for memory.</AbstractText>
        <AbstractText Label="CONCLUSIONS">More sleep leads to better recall.</AbstractText>
      </Abstract>
    </Article>
  </MedlineCitation>
</PubmedArticle>
</PubmedArticleSet>"#;

        let results = parse_articles(xml);
        assert_eq!(results.len(), 2);

        assert_eq!(results[0].title, "Effects of Exercise on Cognition");
        assert_eq!(results[0].url, "https://pubmed.ncbi.nlm.nih.gov/12345678/");
        assert_eq!(
            results[0].summary,
            "Regular exercise improves cognitive function."
        );

        assert_eq!(results[1].title, "Sleep and Memory Consolidation");
        assert_eq!(results[1].url, "https://pubmed.ncbi.nlm.nih.gov/87654321/");
        assert_eq!(
            results[1].summary,
            "Sleep is critical for memory. More sleep leads to better recall."
        );
    }

    #[test]
    fn parse_articles_skips_missing_fields() {
        let xml = "<PubmedArticle><MedlineCitation><PMID Version=\"1\">999</PMID></MedlineCitation></PubmedArticle>";
        let results = parse_articles(xml);
        assert!(results.is_empty());
    }

    #[test]
    fn percent_encode_spaces_and_special() {
        assert_eq!(percent_encode("COVID-19 vaccine"), "COVID-19+vaccine");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    }
}
