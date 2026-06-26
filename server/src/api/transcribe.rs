use std::sync::Arc;

use axum::{
    Json,
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
};

#[derive(Debug)]
pub enum TranscribeApiError {
    Auth(AuthError),
    BadRequest,
    NotConfigured,
    Upstream,
}

impl IntoResponse for TranscribeApiError {
    fn into_response(self) -> Response {
        match self {
            TranscribeApiError::Auth(error) => error.into_response(),
            TranscribeApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            TranscribeApiError::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "no transcription model configured",
            )
                .into_response(),
            TranscribeApiError::Upstream => (
                StatusCode::BAD_GATEWAY,
                "transcription provider request failed",
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for TranscribeApiError {
    fn from(error: AuthError) -> Self {
        TranscribeApiError::Auth(error)
    }
}

#[derive(Serialize)]
pub struct TranscribeResponse {
    text: String,
}

/// Accepts a single audio file (multipart field `file`) and returns its
/// transcription, using the model registered under the `transcription` key.
pub async fn transcribe(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<TranscribeResponse>, TranscribeApiError> {
    auth::authenticated_user(&state, &headers).await?;

    let Some(model) = state.model_config.model_registry.transcription() else {
        return Err(TranscribeApiError::NotConfigured);
    };

    let mut audio: Option<(Vec<u8>, String, String)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| TranscribeApiError::BadRequest)?
    {
        if field.name() != Some("file") {
            continue;
        }
        let name = field
            .file_name()
            .filter(|n| !n.is_empty())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "audio.webm".to_string());
        let mime_type = field
            .content_type()
            .filter(|ct| !ct.is_empty())
            .map(|ct| ct.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|_| TranscribeApiError::BadRequest)?;
        audio = Some((bytes.to_vec(), name, mime_type));
        break;
    }

    let Some((bytes, name, mime_type)) = audio else {
        return Err(TranscribeApiError::BadRequest);
    };
    if bytes.is_empty() {
        return Err(TranscribeApiError::BadRequest);
    }

    let transcription = model
        .api
        .transcribe(&model.token, &bytes, &name, &mime_type, &model.model_name)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "transcription request failed");
            TranscribeApiError::Upstream
        })?;

    Ok(Json(TranscribeResponse {
        text: transcription.text.trim().to_string(),
    }))
}
