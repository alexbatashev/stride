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
    model_registry::{self, UserModelInput, UserModelSummary},
};

#[derive(Debug)]
pub enum UserModelsApiError {
    Auth(AuthError),
    BadRequest(String),
    NotFound,
    Internal,
}

impl IntoResponse for UserModelsApiError {
    fn into_response(self) -> Response {
        match self {
            UserModelsApiError::Auth(error) => error.into_response(),
            UserModelsApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
            }
            UserModelsApiError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "model not found"})),
            )
                .into_response(),
            UserModelsApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for UserModelsApiError {
    fn from(error: AuthError) -> Self {
        UserModelsApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<UserModelSummary>>, UserModelsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::list_user_models(&state.db, owner)
        .await
        .map(Json)
        .map_err(|_| UserModelsApiError::Internal)
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<UserModelInput>,
) -> Result<(StatusCode, Json<UserModelSummary>), UserModelsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::create_user_model(
        &state.db,
        state.clock.as_ref(),
        state.id_gen.as_ref(),
        owner,
        request,
    )
    .await
    .map(|model| (StatusCode::CREATED, Json(model)))
    .map_err(map_error)
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, UserModelsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::delete_user_model(&state.db, owner, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_error)
}

fn map_error(error: anyhow::Error) -> UserModelsApiError {
    let message = error.to_string();
    if message.contains("already exists")
        || message.contains("required")
        || message.contains("invalid")
        || message.contains("must")
    {
        UserModelsApiError::BadRequest(message)
    } else if message.contains("not found") {
        UserModelsApiError::NotFound
    } else {
        UserModelsApiError::Internal
    }
}
