use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    model_registry::{self, ProviderInput, ProviderSummary},
};

#[derive(Debug)]
pub enum ProvidersApiError {
    Auth(AuthError),
    BadRequest(String),
    NotFound,
    Internal,
}

impl IntoResponse for ProvidersApiError {
    fn into_response(self) -> Response {
        match self {
            ProvidersApiError::Auth(error) => error.into_response(),
            ProvidersApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
            }
            ProvidersApiError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "provider not found"})),
            )
                .into_response(),
            ProvidersApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for ProvidersApiError {
    fn from(error: AuthError) -> Self {
        ProvidersApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderSummary>>, ProvidersApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::list_providers(&state.db, owner)
        .await
        .map(Json)
        .map_err(|_| ProvidersApiError::Internal)
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<ProviderInput>,
) -> Result<(StatusCode, Json<ProviderSummary>), ProvidersApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::create_provider(
        &state.db,
        &state.cipher,
        state.clock.as_ref(),
        state.id_gen.as_ref(),
        owner,
        request,
    )
    .await
    .map(|provider| (StatusCode::CREATED, Json(provider)))
    .map_err(map_error)
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ProvidersApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::delete_provider(&state.db, owner, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_error)
}

fn map_error(error: anyhow::Error) -> ProvidersApiError {
    let message = error.to_string();
    if message.contains("already exists")
        || message.contains("required")
        || message.contains("unsupported")
        || message.contains("must")
    {
        ProvidersApiError::BadRequest(message)
    } else if message.contains("not found") {
        ProvidersApiError::NotFound
    } else {
        ProvidersApiError::Internal
    }
}
