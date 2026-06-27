//! Google account linking via a standard OAuth 2.0 / OIDC client.
//!
//! The browser is sent to Google's authorize page, returns to [`callback`] with
//! a short-lived `code`, and the server exchanges it for an access token (and,
//! on first consent, a refresh token). Those tokens are stored and later
//! forwarded to the configured Google MCP server (see [`crate::google`]). The
//! signed-in user is recovered from the `state` parameter because Google
//! redirects back without the session cookie.

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
    config::Google,
    db::{google_connections, google_oauth_states},
};

/// How long a pending OAuth `state` token stays valid.
const STATE_TTL_SECONDS: i64 = 600;
const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";

#[derive(Serialize)]
pub struct GoogleSettingsResponse {
    configured: bool,
    connected: bool,
    email: Option<String>,
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
pub enum GoogleApiError {
    Auth(AuthError),
    NotConfigured,
    Internal,
}

impl IntoResponse for GoogleApiError {
    fn into_response(self) -> Response {
        match self {
            GoogleApiError::Auth(error) => error.into_response(),
            GoogleApiError::NotConfigured => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Google is not configured on this server"})),
            )
                .into_response(),
            GoogleApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for GoogleApiError {
    fn from(error: AuthError) -> Self {
        GoogleApiError::Auth(error)
    }
}

pub async fn settings(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<GoogleSettingsResponse>, GoogleApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    let email = google_connections::select_cols((google_connections::email,))
        .where_(google_connections::user_id.eq(user_id))
        .all(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?
        .into_iter()
        .next()
        .map(|(email,)| email);

    Ok(Json(GoogleSettingsResponse {
        // Tools are only exposed when both OAuth credentials and an MCP endpoint
        // are configured, so reflect that in `configured`.
        configured: google_config(&state).is_some_and(|google| google.mcp_url().is_some()),
        connected: email.is_some(),
        email,
    }))
}

/// Mint a one-time `state`, record it against the user, and return the Google
/// authorize URL the browser should navigate to.
pub async fn authorize(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<AuthorizeResponse>, GoogleApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    let google = google_config(&state).ok_or(GoogleApiError::NotConfigured)?;
    let client_id = google
        .read_client_id()
        .ok_or(GoogleApiError::NotConfigured)?;
    let redirect_uri = redirect_uri(&state).ok_or(GoogleApiError::NotConfigured)?;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    // Drop any stale states for this user before recording the new one so the
    // table cannot grow with abandoned flows.
    google_oauth_states::delete()
        .where_(google_oauth_states::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?;
    google_oauth_states::insert()
        .state(token.as_str())
        .user_id(user_id)
        .expires_at(now() + STATE_TTL_SECONDS)
        .execute(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?;

    // `access_type=offline` + `prompt=consent` ensure a refresh token is issued
    // so the agent keeps working after the access token expires.
    let url = format!(
        "{AUTHORIZE_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}\
         &access_type=offline&prompt=consent&include_granted_scopes=true",
        encode(&client_id),
        encode(&redirect_uri),
        encode(google.scopes()),
        encode(&token),
    );
    Ok(Json(AuthorizeResponse { url }))
}

/// Google redirects the browser here. Recover the user from `state`, exchange the
/// code, store the connection, and bounce back to the settings page.
pub async fn callback(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    match complete(&state, params).await {
        Ok(()) => Redirect::to("/settings"),
        Err(error) => {
            tracing::warn!(%error, "Google OAuth callback failed");
            Redirect::to("/settings?google=error")
        }
    }
}

pub async fn disconnect(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<StatusCode, GoogleApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    google_connections::delete()
        .where_(google_connections::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete(state: &ServerState, params: CallbackParams) -> Result<(), String> {
    let code = params.code.ok_or("missing code")?;
    let token = params.state.ok_or("missing state")?;

    let user_id = consume_state(state, &token).await?;
    let google = google_config(state)
        .cloned()
        .ok_or("Google is not configured")?;
    let client_id = google.read_client_id().ok_or("missing client id")?;
    let client_secret = google.read_client_secret().ok_or("missing client secret")?;
    let redirect = redirect_uri(state).ok_or("missing public_url")?;

    let tokens = exchange_code(&client_id, &client_secret, &code, &redirect).await?;
    let (google_user_id, email) = fetch_user(&tokens.access_token).await?;
    let scope = google.scopes().to_string();
    let expires_at = now() + tokens.expires_in;

    // Tokens are bound to the row id as associated data, so encrypt under the id
    // we are about to insert.
    let id = Uuid::now_v7();
    let access_ciphertext = state.cipher.encrypt(id, &tokens.access_token)?;
    let refresh_ciphertext = tokens
        .refresh_token
        .as_deref()
        .map(|refresh| state.cipher.encrypt(id, refresh))
        .transpose()?;

    // A user may relink, and a Google account may move between users; clear both
    // sides of the unique constraints before inserting the fresh row.
    google_connections::delete()
        .where_(
            google_connections::user_id
                .eq(user_id)
                .or(google_connections::google_user_id.eq(google_user_id.as_str())),
        )
        .execute(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    google_connections::insert()
        .id(id)
        .user_id(user_id)
        .google_user_id(google_user_id.as_str())
        .email(email.as_str())
        .access_token(access_ciphertext.as_str())
        .refresh_token(refresh_ciphertext.as_deref())
        .scope(Some(scope.as_str()))
        .expires_at(expires_at)
        .connected_at(now())
        .execute(&state.db)
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

/// Look up and delete the pending `state`, returning the user that started the
/// flow. Expired tokens are rejected.
async fn consume_state(state: &ServerState, token: &str) -> Result<Uuid, String> {
    let row = google_oauth_states::select()
        .where_(google_oauth_states::state.eq(token))
        .all(&state.db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .next()
        .ok_or("unknown OAuth state")?;
    google_oauth_states::delete()
        .where_(google_oauth_states::state.eq(token))
        .execute(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    if row.expires_at < now() {
        return Err("OAuth state expired".to_string());
    }
    Ok(row.user_id)
}

struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let body = form_encode(&[
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
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
        .map_err(|_| "Google token request timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("Google token endpoint returned status {status}"));
    }

    let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("Google token exchange failed: {error}"));
    }
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "Google token response missing access_token".to_string())?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_string);
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    Ok(TokenResponse {
        access_token,
        refresh_token,
        expires_in,
    })
}

async fn fetch_user(access_token: &str) -> Result<(String, String), String> {
    let req = Request::builder()
        .method("GET")
        .uri(USERINFO_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .body(Full::new(Bytes::new()))
        .map_err(|error| error.to_string())?;

    let (status, body) = timeout(Duration::from_secs(30), tinynet::send_request(req))
        .await
        .map_err(|_| "Google userinfo request timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("Google userinfo endpoint returned status {status}"));
    }

    let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    let sub = value
        .get("sub")
        .and_then(Value::as_str)
        .ok_or_else(|| "Google userinfo response missing sub".to_string())?
        .to_string();
    let email = value
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok((sub, email))
}

fn google_config(state: &ServerState) -> Option<&Google> {
    state
        .config
        .server
        .as_ref()
        .and_then(|server| server.google.as_ref())
        .filter(|google| google.is_configured())
}

fn redirect_uri(state: &ServerState) -> Option<String> {
    state
        .config
        .public_url()
        .map(|base| format!("{base}/api/settings/google/callback"))
}

/// Percent-encode `application/x-www-form-urlencoded` key/value pairs.
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
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
        assert_eq!(
            encode("https://www.googleapis.com/auth/calendar"),
            "https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar"
        );
        assert_eq!(encode("Az0-._~"), "Az0-._~");
    }

    #[test]
    fn form_encodes_pairs() {
        assert_eq!(
            form_encode(&[("grant_type", "authorization_code"), ("code", "a/b c")]),
            "grant_type=authorization_code&code=a%2Fb%20c"
        );
    }
}
