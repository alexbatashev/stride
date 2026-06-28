//! Google account linking via OAuth 2.0 / OpenID Connect.
//!
//! The browser is sent to Google's consent screen, returns to [`callback`] with a
//! short-lived `code`, and the server exchanges it for access and refresh tokens.
//! Identity comes from the OIDC `id_token`. The signed-in user is recovered from
//! the `state` parameter because Google redirects back without the session cookie.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

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
    let token = hex::encode(bytes);

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
    if row.expires_at < now() {
        return Err("OAuth state expired".to_string());
    }
    Ok(row.user_id)
}

fn google_client_id(state: &ServerState) -> Option<String> {
    state.config.google().read_client_id()
}

fn google_scopes(state: &ServerState) -> Option<String> {
    Some(state.config.google().scopes().to_string())
}

fn redirect_uri(state: &ServerState) -> Option<String> {
    state
        .config
        .public_url()
        .map(|base| format!("{base}/api/settings/google/callback"))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs() as i64
}

/// Build the server's [`GoogleService`] when OAuth credentials are configured.
pub fn build_service(
    config: &crate::config::Config,
    db: &minisql::ConnectionPool,
    cipher: &crate::crypto::SecretCipher,
) -> Option<GoogleService> {
    let google = config.google();
    if !google.is_configured() {
        return None;
    }
    Some(GoogleService::new(
        db.clone(),
        cipher.clone(),
        google.read_client_id()?,
        google.read_client_secret()?,
    ))
}
