//! Google account linking via OAuth 2.0 / OpenID Connect.
//!
//! The browser is sent to Google's consent screen, returns to [`callback`] with a
//! short-lived `code`, and the server exchanges it for access and refresh tokens.
//! Identity comes from the OIDC `id_token`. The signed-in user is recovered from
//! the `state` parameter because Google redirects back without the session cookie.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::google_oauth_states,
    google::{AUTHORIZE_URL, GoogleService, percent_encode},
};

/// How long a pending OAuth `state` token stays valid.
const STATE_TTL_SECONDS: i64 = 600;

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
pub struct AuthorizeParams {
    return_to: Option<String>,
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
    let email = match state.google_service.as_ref() {
        Some(service) => service.linked_email(user_id).await,
        None => None,
    };
    Ok(Json(GoogleSettingsResponse {
        configured: state.google_service.is_some(),
        connected: email.is_some(),
        email,
    }))
}

/// Mint a one-time `state`, record it against the user, and return the Google
/// consent URL the browser should navigate to.
pub async fn authorize(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Query(params): Query<AuthorizeParams>,
) -> Result<Json<AuthorizeResponse>, GoogleApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    state
        .google_service
        .as_ref()
        .ok_or(GoogleApiError::NotConfigured)?;
    let client_id = google_client_id(&state).ok_or(GoogleApiError::NotConfigured)?;
    let scopes = google_scopes(&state).ok_or(GoogleApiError::NotConfigured)?;
    let redirect_uri = redirect_uri(&state).ok_or(GoogleApiError::NotConfigured)?;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = oauth_state(&hex::encode(bytes), params.return_to.as_deref());

    google_oauth_states::delete()
        .where_(google_oauth_states::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?;
    google_oauth_states::insert()
        .state(token.as_str())
        .user_id(user_id)
        .expires_at(state.clock.now_unix_secs() + STATE_TTL_SECONDS)
        .execute(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?;

    // access_type=offline + prompt=consent guarantees a refresh token even on
    // re-link; include_granted_scopes keeps any previously granted scopes.
    let url = format!(
        "{AUTHORIZE_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent&include_granted_scopes=true",
        percent_encode(&client_id),
        percent_encode(&redirect_uri),
        percent_encode(&scopes),
        percent_encode(&token),
    );
    Ok(Json(AuthorizeResponse { url }))
}

/// Google redirects the browser here. Recover the user from `state`, exchange the
/// code, store the connection, and bounce back to the settings page.
pub async fn callback(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    let return_to = return_path_from_state(params.state.as_deref());
    match complete(&state, params).await {
        Ok(()) => Redirect::to(&settings_return_url(&return_to, "google", "connected")),
        Err(error) => {
            tracing::warn!(%error, "Google OAuth callback failed");
            Redirect::to(&settings_return_url(&return_to, "google", "error"))
        }
    }
}

fn oauth_state(token: &str, return_to: Option<&str>) -> String {
    match return_to.filter(|path| valid_return_path(path)) {
        Some(path) => format!("{token}:{path}"),
        None => token.to_string(),
    }
}

fn return_path_from_state(state: Option<&str>) -> String {
    state
        .and_then(|value| value.split_once(':').map(|(_, path)| path))
        .filter(|path| valid_return_path(path))
        .unwrap_or("/threads")
        .to_string()
}

fn valid_return_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && path.len() <= 2048
        && !path.chars().any(char::is_control)
}

fn settings_return_url(return_to: &str, provider: &str, status: &str) -> String {
    let (path, fragment) = return_to.split_once('#').unwrap_or((return_to, ""));
    let separator = if path.contains('?') { '&' } else { '?' };
    let suffix = if fragment.is_empty() {
        String::new()
    } else {
        format!("#{fragment}")
    };
    format!("{path}{separator}settings=open&section=connections&{provider}={status}{suffix}")
}

pub async fn disconnect(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<StatusCode, GoogleApiError> {
    let user_id = auth::authenticated_user(&state, &headers).await?;
    crate::db::google_connections::delete()
        .where_(crate::db::google_connections::user_id.eq(user_id))
        .execute(&state.db)
        .await
        .map_err(|_| GoogleApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete(state: &ServerState, params: CallbackParams) -> Result<(), String> {
    let code = params.code.ok_or("missing code")?;
    let token = params.state.ok_or("missing state")?;
    let service = state
        .google_service
        .as_ref()
        .ok_or("Google is not configured")?;

    let user_id = consume_state(state, &token).await?;
    let redirect = redirect_uri(state).ok_or("missing public_url")?;
    let tokens = service.exchange_code(&code, &redirect).await?;
    service.store_connection(user_id, tokens).await
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
    if row.expires_at < state.clock.now_unix_secs() {
        return Err("OAuth state expired".to_string());
    }
    Ok(row.user_id)
}

fn google_client_id(state: &ServerState) -> Option<String> {
    google_config(state).and_then(|google| google.read_client_id())
}

fn google_scopes(state: &ServerState) -> Option<String> {
    google_config(state).map(|google| google.scopes().to_string())
}

fn google_config(state: &ServerState) -> Option<&crate::config::Google> {
    state
        .config
        .server
        .as_ref()
        .and_then(|server| server.google.as_ref())
}

fn redirect_uri(state: &ServerState) -> Option<String> {
    state
        .config
        .public_url()
        .map(|base| format!("{base}/api/settings/google/callback"))
}

/// Build the server's [`GoogleService`] when OAuth credentials are configured.
pub fn build_service(
    config: &crate::config::Config,
    db: &minisql::ConnectionPool,
    cipher: &crate::crypto::SecretCipher,
    clock: std::sync::Arc<dyn stride_agent::Clock>,
    id_gen: std::sync::Arc<dyn stride_agent::IdGen>,
) -> Option<GoogleService> {
    let google = config
        .server
        .as_ref()
        .and_then(|server| server.google.as_ref())
        .filter(|google| google.is_configured())?;
    Some(GoogleService::with_clock(
        db.clone(),
        cipher.clone(),
        google.read_client_id()?,
        google.read_client_secret()?,
        clock,
        id_gen,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_state_restores_only_internal_settings_paths() {
        let state = oauth_state("token", Some("/automations"));
        assert_eq!(return_path_from_state(Some(&state)), "/automations");
        assert_eq!(oauth_state("token", Some("https://example.com")), "token");
        assert_eq!(oauth_state("token", Some("//example.com")), "token");
    }

    #[test]
    fn oauth_return_reopens_connections() {
        assert_eq!(
            settings_return_url("/automations", "google", "error"),
            "/automations?settings=open&section=connections&google=error"
        );
    }
}
