use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    model_registry::{self, ModelSummary},
};

#[derive(Debug)]
pub enum ModelsApiError {
    Auth(AuthError),
    Internal,
}

impl IntoResponse for ModelsApiError {
    fn into_response(self) -> Response {
        match self {
            ModelsApiError::Auth(error) => error.into_response(),
            ModelsApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
                .into_response(),
        }
    }
}

impl From<AuthError> for ModelsApiError {
    fn from(error: AuthError) -> Self {
        ModelsApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ModelSummary>>, ModelsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    model_registry::list_available_models(&state.config, &state.db, owner)
        .await
        .map(Json)
        .map_err(|_| ModelsApiError::Internal)
}
