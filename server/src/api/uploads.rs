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

/// Staging area for files uploaded before a thread exists. Uploads land here and
/// are moved into a thread's workspace when one is created or messaged.

#[derive(Debug)]
pub enum UploadsApiError {
    Auth(AuthError),
    BadRequest,
    Internal,
}

impl IntoResponse for UploadsApiError {
    fn into_response(self) -> Response {
        match self {
            UploadsApiError::Auth(error) => error.into_response(),
            UploadsApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            UploadsApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for UploadsApiError {
    fn from(error: AuthError) -> Self {
        UploadsApiError::Auth(error)
    }
}

#[derive(Serialize)]
pub struct StagedUploadResponse {
    id: String,
    name: String,
    size: usize,
}

#[derive(Serialize)]
pub struct UploadResponse {
    files: Vec<StagedUploadResponse>,
}

pub async fn upload(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, UploadsApiError> {
    let Some(ref vfs) = state.vfs else {
        return Err(UploadsApiError::Internal);
    };
    let owner = auth::authenticated_user(&state, &headers).await?;

    let mut uploaded = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| UploadsApiError::BadRequest)?
    {
        let name = field
            .file_name()
            .filter(|n| !n.is_empty())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "file".to_string());
        let mime_type = field
            .content_type()
            .filter(|ct| !ct.is_empty())
            .map(|ct| ct.to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|_| UploadsApiError::BadRequest)?;
        let staged = vfs
            .stage_upload(owner, &name, mime_type.as_deref(), &bytes)
            .await
            .map_err(|_| UploadsApiError::Internal)?;

        uploaded.push(StagedUploadResponse {
            id: staged.id.to_string(),
            name: staged.name,
            size: staged.size as usize,
        });
    }

    Ok(Json(UploadResponse { files: uploaded }))
}
