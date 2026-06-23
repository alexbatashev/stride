//! GitHub account linking via a standard OAuth App.
//!
//! The browser is sent to GitHub's authorize page, returns to [`callback`] with a
//! short-lived `code`, and the server exchanges it for a user access token. That
//! token is stored and later forwarded to the hosted GitHub MCP server (see
//! [`crate::github`]). The signed-in user is recovered from the `state` parameter
//! because GitHub redirects back without the session cookie.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    config::GitHub,
    db::{github_connections, github_oauth_states},
};

/// How long a pending OAuth `state` token stays valid.
const STATE_TTL_SECONDS: i64 = 600;
const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";

#[derive(Serialize)]
pub struct GitHubSettingsResponse {
    configured: bool,
    connected: bool,
    login: Option<String>,
}

#[derive(Serialize)]
pub struct AuthorizeResponse {
    url: String,
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
}

#[derive(Debug)]
pub enum GitHubApiError {
    Auth(AuthError),
    NotConfigured,
    Internal,
}

impl IntoResponse for GitHubApiError {
    fn into_response(self) -> Response {
        match self {
            GitHubApiError::Auth(error) => error.into_response(),
            GitHubApiError::NotConfigured => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "GitHub is not configured on this server"})),
            )
                .into_response(),
            GitHubApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for GitHubApiError {
    fn from(error: AuthError) -> Self {
        GitHubApiError::Auth(error)
    }
}

pub async fn settings(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<GitHubSettingsResponse>, GitHubApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    let login = github_connections::select_cols((github_connections::login,))
        .where_(github_connections::user_id.eq(user_id))
        .all(&state.db)
        .await
        .map_err(|_| GitHubApiError::Internal)?
        .into_iter()
        .next()
        .map(|(login,)| login);

    Ok(Json(GitHubSettingsResponse {
        configured: github_config(&state).is_some(),
        connected: login.is_some(),
        login,
    }))
}

/// Mint a one-time `state`, record it against the user, and return the GitHub
/// authorize URL the browser should navigate to.
pub async fn authorize(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<AuthorizeResponse>, GitHubApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    let github = github_config(&state).ok_or(GitHubApiError::NotConfigured)?;
    let client_id = github
        .read_client_id()
        .ok_or(GitHubApiError::NotConfigured)?;
    let redirect_uri = redirect_uri(&state).ok_or(GitHubApiError::NotConfigured)?;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    // Drop any stale states for this user before recording the new one so the
    // table cannot grow with abandoned flows.
    github_oauth_states::delete()
        .where_(github_oauth_states::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| GitHubApiError::Internal)?;
    github_oauth_states::insert()
        .state(token.as_str())
        .user_id(user_id)
        .expires_at(now() + STATE_TTL_SECONDS)
        .execute(&state.db)
        .await
        .map_err(|_| GitHubApiError::Internal)?;

    let url = format!(
        "{AUTHORIZE_URL}?client_id={}&redirect_uri={}&scope={}&state={}&allow_signup=false",
        encode(&client_id),
        encode(&redirect_uri),
        encode(github.scopes()),
        encode(&token),
    );
    Ok(Json(AuthorizeResponse { url }))
}

/// GitHub redirects the browser here. Recover the user from `state`, exchange the
/// code, store the connection, and bounce back to the settings page.
pub async fn callback(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    match complete(&state, params).await {
        Ok(()) => Redirect::to("/settings"),
        Err(error) => {
            tracing::warn!(%error, "GitHub OAuth callback failed");
            Redirect::to("/settings?github=error")
        }
    }
}

pub async fn disconnect(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<StatusCode, GitHubApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    github_connections::delete()
        .where_(github_connections::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| GitHubApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete(state: &ServerState, params: CallbackParams) -> Result<(), String> {
    let code = params.code.ok_or("missing code")?;
    let token = params.state.ok_or("missing state")?;

    let user_id = consume_state(state, &token).await?;
    let github = github_config(state)
        .cloned()
        .ok_or("GitHub is not configured")?;
    let client_id = github.read_client_id().ok_or("missing client id")?;
    let client_secret = github.read_client_secret().ok_or("missing client secret")?;
    let redirect = redirect_uri(state).ok_or("missing public_url")?;

    let access_token = exchange_code(&client_id, &client_secret, &code, &redirect).await?;
    let (github_user_id, login) = fetch_user(&access_token).await?;
    let scope = github.scopes().to_string();

    // A user may relink, and a GitHub account may move between users; clear both
    // sides of the unique constraints before inserting the fresh row.
    github_connections::delete()
        .where_(
            github_connections::user_id
                .eq(user_id)
                .or(github_connections::github_user_id.eq(github_user_id)),
        )
        .execute(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    github_connections::insert()
        .id(Uuid::now_v7())
        .user_id(user_id)
        .github_user_id(github_user_id)
        .login(login.as_str())
        .access_token(access_token.as_str())
        .scope(Some(scope.as_str()))
        .connected_at(now())
        .execute(&state.db)
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

/// Look up and delete the pending `state`, returning the user that started the
/// flow. Expired tokens are rejected.
async fn consume_state(state: &ServerState, token: &str) -> Result<Uuid, String> {
    let row = github_oauth_states::select()
        .where_(github_oauth_states::state.eq(token))
        .all(&state.db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .next()
        .ok_or("unknown OAuth state")?;
    github_oauth_states::delete()
        .where_(github_oauth_states::state.eq(token))
        .execute(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    if row.expires_at < now() {
        return Err("OAuth state expired".to_string());
    }
    Ok(row.user_id)
}

async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    let body = serde_json::to_vec(&json!({
        "client_id": client_id,
        "client_secret": client_secret,
        "code": code,
        "redirect_uri": redirect_uri,
    }))
    .map_err(|error| error.to_string())?;
    let req = Request::builder()
        .method("POST")
        .uri(TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("User-Agent", "stride")
        .body(Full::new(Bytes::from(body)))
        .map_err(|error| error.to_string())?;

    let (status, body) = timeout(Duration::from_secs(30), tinynet::send_request(req))
        .await
        .map_err(|_| "GitHub token request timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("GitHub token endpoint returned status {status}"));
    }

    let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("GitHub token exchange failed: {error}"));
    }
    value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "GitHub token response missing access_token".to_string())
}

async fn fetch_user(access_token: &str) -> Result<(i64, String), String> {
    let req = Request::builder()
        .method("GET")
        .uri(USER_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "stride")
        .body(Full::new(Bytes::new()))
        .map_err(|error| error.to_string())?;

    let (status, body) = timeout(Duration::from_secs(30), tinynet::send_request(req))
        .await
        .map_err(|_| "GitHub user request timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("GitHub user endpoint returned status {status}"));
    }

    let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| "GitHub user response missing id".to_string())?;
    let login = value
        .get("login")
        .and_then(Value::as_str)
        .ok_or_else(|| "GitHub user response missing login".to_string())?
        .to_string();
    Ok((id, login))
}

fn github_config(state: &ServerState) -> Option<&GitHub> {
    state
        .config
        .server
        .as_ref()
        .and_then(|server| server.github.as_ref())
        .filter(|github| github.is_configured())
}

fn redirect_uri(state: &ServerState) -> Option<String> {
    state
        .config
        .public_url()
        .map(|base| format!("{base}/api/settings/github/callback"))
}

/// Percent-encode a query parameter value, leaving only the RFC 3986 unreserved
/// characters untouched.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_query_characters() {
        assert_eq!(encode("repo read:org"), "repo%20read%3Aorg");
        assert_eq!(
            encode("https://host/api/settings/github/callback"),
            "https%3A%2F%2Fhost%2Fapi%2Fsettings%2Fgithub%2Fcallback"
        );
        assert_eq!(encode("Az0-._~"), "Az0-._~");
    }
}
