//! Per-user connection to a Google MCP server.
//!
//! Once a user links their Google account (see [`crate::api::google`]), their
//! access token is decrypted and forwarded as a bearer credential to the
//! configured Google MCP server, and every advertised tool is exposed to the
//! agent under the `google` prefix.
//!
//! Unlike GitHub OAuth-App tokens, Google access tokens are short lived
//! (~1 hour). When a token is expired (or about to be) and a refresh token is
//! stored, it is refreshed and persisted before connecting.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use minisql::ConnectionPool;
use serde_json::Value;
use stride_agent::mcp::{self, McpTool};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{crypto::SecretCipher, db::google_connections};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Refresh a little before expiry so a token does not lapse mid-request.
const EXPIRY_BUFFER_SECONDS: i64 = 60;

/// Everything a worker needs to attach a user's Google MCP tools: where the
/// server lives, the cipher that unseals stored tokens, and the OAuth client
/// credentials needed to refresh an expired access token.
#[derive(Clone)]
pub struct GoogleRuntime {
    pub mcp_url: String,
    pub cipher: SecretCipher,
    pub client_id: String,
    pub client_secret: String,
}

/// Connect to the Google MCP server on behalf of `user`, returning one tool per
/// advertised capability. Yields an empty list when the user has not linked an
/// account, the token cannot be decrypted or refreshed, or the server is
/// unreachable.
pub async fn connect_user_google_mcp(
    db: &ConnectionPool,
    user: Uuid,
    runtime: &GoogleRuntime,
) -> Vec<McpTool> {
    let connection = match google_connections::select()
        .where_(google_connections::user_id.eq(user))
        .all(db)
        .await
    {
        Ok(rows) => rows.into_iter().next(),
        Err(error) => {
            tracing::warn!(%error, user_id = %user, "failed to load Google connection");
            return Vec::new();
        }
    };
    let Some(connection) = connection else {
        return Vec::new();
    };

    let token = match resolve_access_token(db, runtime, &connection).await {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(%error, user_id = %user, "failed to resolve Google access token");
            return Vec::new();
        }
    };

    let server = mcp::McpServer {
        url: runtime.mcp_url.clone(),
        headers: vec![("Authorization".to_string(), format!("Bearer {token}"))],
    };
    match mcp::connect("google", server).await {
        Ok(tools) => {
            tracing::info!(
                user_id = %user,
                count = tools.len(),
                "connected to Google MCP server"
            );
            tools
        }
        Err(error) => {
            tracing::warn!(%error, user_id = %user, "failed to connect to Google MCP server");
            Vec::new()
        }
    }
}

/// Return a usable access token, refreshing and persisting a new one when the
/// stored token is expired (or within the buffer) and a refresh token exists.
async fn resolve_access_token(
    db: &ConnectionPool,
    runtime: &GoogleRuntime,
    connection: &google_connections::Row,
) -> Result<String, String> {
    let access_token = runtime
        .cipher
        .decrypt(connection.id, &connection.access_token)?;

    if connection.expires_at - EXPIRY_BUFFER_SECONDS > now() {
        return Ok(access_token);
    }

    let Some(refresh_ciphertext) = connection.refresh_token.as_deref() else {
        // No refresh token: best effort with whatever we have.
        return Ok(access_token);
    };
    let refresh_token = runtime.cipher.decrypt(connection.id, refresh_ciphertext)?;

    let (new_access, expires_in) = refresh_access_token(runtime, &refresh_token).await?;
    let expires_at = now() + expires_in;
    let access_ciphertext = runtime.cipher.encrypt(connection.id, &new_access)?;

    google_connections::update()
        .access_token(access_ciphertext)
        .expires_at(expires_at)
        .where_(google_connections::id.eq(connection.id))
        .execute(db)
        .await
        .map_err(|error| error.to_string())?;

    Ok(new_access)
}

async fn refresh_access_token(
    runtime: &GoogleRuntime,
    refresh_token: &str,
) -> Result<(String, i64), String> {
    let body = form_encode(&[
        ("client_id", &runtime.client_id),
        ("client_secret", &runtime.client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]);
    let req = Request::builder()
        .method("POST")
        .uri(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|error| error.to_string())?;

    let (status, body) = timeout(Duration::from_secs(30), tinynet::send_request(req))
        .await
        .map_err(|_| "Google token refresh timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("Google token refresh returned status {status}"));
    }

    let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "Google token response missing access_token".to_string())?;
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    Ok((access_token, expires_in))
}

/// Percent-encode `application/x-www-form-urlencoded` key/value pairs.
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode a value, leaving only the RFC 3986 unreserved characters.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs() as i64
}
