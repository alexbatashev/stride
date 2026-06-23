use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use friday_agent::mcp::{self, McpTool};
use hyper::{
    Uri,
    header::{HeaderName, HeaderValue},
};
use minisql::ConnectionPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::mcp_servers;

#[derive(Clone, Debug, Deserialize)]
pub struct McpServerInput {
    pub name: String,
    pub url: String,
    pub bearer_token: Option<String>,
    pub headers_json: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct McpServerSummary {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub created_at: i64,
    pub header_names: Vec<String>,
    pub has_authorization: bool,
}

pub fn summarize(row: mcp_servers::Row) -> McpServerSummary {
    let headers = parse_stored_headers(row.headers_json.as_deref()).unwrap_or_default();
    let mut header_names: Vec<_> = headers
        .keys()
        .filter(|name| !name.eq_ignore_ascii_case("authorization"))
        .cloned()
        .collect();
    header_names.sort();
    McpServerSummary {
        id: row.id.to_string(),
        name: row.name,
        url: row.url,
        enabled: row.enabled,
        created_at: row.created_at,
        has_authorization: headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization")),
        header_names,
    }
}

pub fn normalize_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.len() < 2 || name.len() > 48 {
        return Err("name must be 2-48 characters".to_string());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("name is required".to_string());
    };
    if !first.is_ascii_alphabetic() {
        return Err("name must start with a letter".to_string());
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return Err("name may only contain letters, numbers, and underscores".to_string());
    }
    Ok(name.to_string())
}

pub fn normalize_url(value: &str) -> Result<String, String> {
    let url = value.trim();
    if url.is_empty() || url.contains(['\r', '\n']) {
        return Err("invalid MCP server URL".to_string());
    }
    let uri: Uri = url
        .parse()
        .map_err(|_| "invalid MCP server URL".to_string())?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| "MCP server URL must include http or https".to_string())?;
    if scheme != "http" && scheme != "https" {
        return Err("MCP server URL must use http or https".to_string());
    }
    let host = uri
        .host()
        .ok_or_else(|| "MCP server URL must include a host".to_string())?;
    if !is_public_host(host) {
        return Err("MCP server URL must point to a public host".to_string());
    }
    Ok(url.to_string())
}

fn is_public_host(host: &str) -> bool {
    let unbracketed = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);

    if let Ok(ipv4) = unbracketed.parse::<Ipv4Addr>() {
        return is_public_ipv4(&ipv4);
    }
    if let Ok(ipv6) = unbracketed.parse::<Ipv6Addr>() {
        return is_public_ipv6(&ipv6);
    }
    is_public_hostname(unbracketed)
}

fn is_public_hostname(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    if host == "localhost" {
        return false;
    }
    !(host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal"))
}

fn is_public_ipv4(addr: &Ipv4Addr) -> bool {
    !(addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_unspecified()
        || addr.is_broadcast())
}

fn is_public_ipv6(addr: &Ipv6Addr) -> bool {
    if addr.is_loopback() || addr.is_unspecified() {
        return false;
    }
    let octets = addr.octets();
    if octets[0] & 0xfe == 0xfc {
        return false;
    }
    let segments = addr.segments();
    if segments[0] & 0xffc0 == 0xfe80 {
        return false;
    }
    true
}

pub fn headers_json(input: &McpServerInput) -> Result<Option<String>, String> {
    let mut headers = parse_input_headers(input.headers_json.as_deref())?;
    if let Some(token) = input
        .bearer_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    }
    if headers.is_empty() {
        return Ok(None);
    }
    let json = serde_json::to_string(&headers).map_err(|_| "invalid headers".to_string())?;
    Ok(Some(json))
}

pub async fn connect_user_mcp_servers(db: &ConnectionPool, owner: Uuid) -> Vec<McpTool> {
    let rows = match mcp_servers::select()
        .where_(
            mcp_servers::owner
                .eq(owner)
                .and(mcp_servers::enabled.eq(true)),
        )
        .order_by_asc(mcp_servers::name)
        .all(db)
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, user_id = %owner, "failed to load user MCP servers");
            return Vec::new();
        }
    };

    let mut tools = Vec::new();
    for row in rows {
        let headers = match parse_stored_headers(row.headers_json.as_deref()) {
            Ok(headers) => headers.into_iter().collect(),
            Err(error) => {
                tracing::warn!(%error, server = %row.name, "skipping invalid user MCP headers");
                continue;
            }
        };
        let server = mcp::McpServer {
            url: row.url.clone(),
            headers,
        };
        let server_name = format!("custom_{}", row.name);
        match mcp::connect(&server_name, server).await {
            Ok(server_tools) => {
                tracing::info!(
                    server = %row.name,
                    user_id = %owner,
                    count = server_tools.len(),
                    "connected to user MCP server"
                );
                tools.extend(server_tools);
            }
            Err(error) => {
                tracing::warn!(
                    server = %row.name,
                    user_id = %owner,
                    %error,
                    "failed to connect to user MCP server"
                );
            }
        }
    }
    tools
}

fn parse_input_headers(value: Option<&str>) -> Result<HashMap<String, String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(HashMap::new());
    };
    parse_headers(value)
}

fn parse_stored_headers(value: Option<&str>) -> Result<HashMap<String, String>, String> {
    let Some(value) = value else {
        return Ok(HashMap::new());
    };
    parse_headers(value)
}

fn parse_headers(value: &str) -> Result<HashMap<String, String>, String> {
    let headers: HashMap<String, String> =
        serde_json::from_str(value).map_err(|_| "headers must be a JSON object".to_string())?;
    for (name, value) in &headers {
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("invalid header name: {name}"))?;
        HeaderValue::from_str(value).map_err(|_| format!("invalid header value: {name}"))?;
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_mcp_names_for_tool_prefixes() {
        assert_eq!(normalize_name("deepwiki").unwrap(), "deepwiki");
        assert_eq!(normalize_name("deep_wiki2").unwrap(), "deep_wiki2");
        assert!(normalize_name("2wiki").is_err());
        assert!(normalize_name("deep-wiki").is_err());
    }

    #[test]
    fn accepts_only_remote_http_urls() {
        assert!(normalize_url("https://mcp.example.com/mcp").is_ok());
        assert!(normalize_url("stdio://server").is_err());
        assert!(normalize_url("/mcp").is_err());
    }

    #[test]
    fn rejects_private_and_internal_targets() {
        assert!(normalize_url("https://mcp.example.com/mcp").is_ok());
        assert!(normalize_url("http://169.254.169.254").is_err());
        assert!(normalize_url("http://localhost").is_err());
        assert!(normalize_url("http://127.0.0.1").is_err());
        assert!(normalize_url("http://10.0.0.5").is_err());
        assert!(normalize_url("http://192.168.1.1").is_err());
        assert!(normalize_url("http://[::1]").is_err());
    }

    #[test]
    fn builds_headers_without_echo_logic() {
        let input = McpServerInput {
            name: "deepwiki".to_string(),
            url: "https://mcp.example.com/mcp".to_string(),
            bearer_token: Some("secret".to_string()),
            headers_json: Some(r#"{"X-Tenant":"acme"}"#.to_string()),
            enabled: None,
        };
        let headers = headers_json(&input).unwrap().unwrap();
        assert!(headers.contains(r#""X-Tenant":"acme""#));
        assert!(headers.contains(r#""Authorization":"Bearer secret""#));
    }
}
