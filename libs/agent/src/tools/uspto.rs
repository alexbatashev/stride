use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use serde::Deserialize;
use serde_json::json;

use super::web_search::{SearchProvider, SearchResult};

const BASE_URL: &str = "https://ppubs.uspto.gov";

#[derive(Deserialize)]
struct SessionBody {
    #[serde(rename = "userCase")]
    user_case: UserCase,
}

#[derive(Deserialize)]
struct UserCase {
    #[serde(rename = "caseId")]
    case_id: i64,
}

#[derive(Deserialize)]
struct SearchResponse {
    patents: Option<Vec<Patent>>,
}

#[derive(Deserialize)]
struct Patent {
    guid: Option<String>,
    #[serde(rename = "patentNumber")]
    patent_number: Option<String>,
    #[serde(rename = "applicationNumber")]
    application_number: Option<String>,
    #[serde(rename = "inventionTitle")]
    invention_title: Option<String>,
    #[serde(rename = "abstractText")]
    abstract_text: Option<String>,
    #[serde(rename = "type")]
    source_type: Option<String>,
}

pub struct UsptoProvider;

#[async_trait(?Send)]
impl SearchProvider for UsptoProvider {
    fn categories(&self) -> &[&str] {
        &["academic"]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let (case_id, access_token) = establish_session().await?;
        search_patents(query, limit, case_id, &access_token).await
    }
}

async fn establish_session() -> Result<(i64, String), String> {
    let req = Request::builder()
        .method("POST")
        .uri(format!("{BASE_URL}/api/users/me/session"))
        .header("content-type", "application/json")
        .header("x-access-token", "null")
        .header("x-requested-with", "XMLHttpRequest")
        .header(
            "user-agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
        )
        .header("origin", BASE_URL)
        .header("referer", format!("{BASE_URL}/pubwebapp/"))
        .body(Full::new(Bytes::from_static(b"-1")))
        .map_err(|e| e.to_string())?;

    let (status, headers, body) = tinynet::send_request_full(req)
        .await
        .map_err(|e| e.to_string())?;

    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }

    let session: SessionBody = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let access_token = headers
        .get("x-access-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok((session.user_case.case_id, access_token))
}

async fn search_patents(
    query: &str,
    limit: usize,
    case_id: i64,
    access_token: &str,
) -> Result<Vec<SearchResult>, String> {
    let search_body = json!({
        "start": 0,
        "pageCount": limit.min(50),
        "sort": "date_publ desc",
        "query": {
            "caseId": case_id,
            "op": "AND",
            "q": query,
            "queryName": query,
            "userEnteredQuery": query,
            "databaseFilters": [
                {"databaseName": "US-PGPUB", "countryCodes": []},
                {"databaseName": "USPAT", "countryCodes": []}
            ],
            "plurals": true,
            "britishEquivalents": true
        }
    });

    let json_bytes = serde_json::to_vec(&search_body).map_err(|e| e.to_string())?;
    let req = Request::builder()
        .method("POST")
        .uri(format!("{BASE_URL}/api/searches/searchWithBeFamily"))
        .header("content-type", "application/json")
        .header("x-access-token", access_token)
        .header("x-requested-with", "XMLHttpRequest")
        .header(
            "user-agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
        )
        .body(Full::new(Bytes::from(json_bytes)))
        .map_err(|e| e.to_string())?;

    let (status, body) = tinynet::send_request(req)
        .await
        .map_err(|e| e.to_string())?;

    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }

    let resp: SearchResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;

    Ok(resp
        .patents
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            let title = p.invention_title?;
            let summary = p.abstract_text.unwrap_or_default();
            let url = patent_url(
                p.guid.as_deref(),
                p.patent_number.as_deref(),
                p.application_number.as_deref(),
                p.source_type.as_deref(),
            );
            Some(SearchResult {
                title,
                url,
                summary,
            })
        })
        .collect())
}

fn patent_url(
    guid: Option<&str>,
    patent_number: Option<&str>,
    application_number: Option<&str>,
    source_type: Option<&str>,
) -> String {
    let source = source_type.unwrap_or("USPAT");
    let number = patent_number.or(application_number).or(guid).unwrap_or("");
    let normalized = number.replace('/', "");
    format!("{BASE_URL}/pubwebapp/external.html?q=({normalized}.pn.)&type={source}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patent_url_uses_patent_number_first() {
        let url = patent_url(
            Some("guid123"),
            Some("US10234567"),
            Some("16/123456"),
            Some("USPAT"),
        );
        assert_eq!(
            url,
            "https://ppubs.uspto.gov/pubwebapp/external.html?q=(US10234567.pn.)&type=USPAT"
        );
    }

    #[test]
    fn patent_url_falls_back_to_application_number() {
        let url = patent_url(Some("guid123"), None, Some("16/123456"), Some("US-PGPUB"));
        assert_eq!(
            url,
            "https://ppubs.uspto.gov/pubwebapp/external.html?q=(16123456.pn.)&type=US-PGPUB"
        );
    }

    #[test]
    fn patent_url_falls_back_to_guid() {
        let url = patent_url(Some("APA16123456"), None, None, None);
        assert_eq!(
            url,
            "https://ppubs.uspto.gov/pubwebapp/external.html?q=(APA16123456.pn.)&type=USPAT"
        );
    }
}
