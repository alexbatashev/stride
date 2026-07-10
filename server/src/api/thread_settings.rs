use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use minisql::{ConnectionPool, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
};

/// Applied when a user has never saved a retention policy.
pub const DEFAULT_ARCHIVE_DAYS: i64 = 14;
pub const DEFAULT_REMOVE_DAYS: i64 = 90;

const MAX_DAYS: i64 = 3650;

/// A user's auto-archive / auto-remove policy. `None` for a field means that
/// sweep is disabled; `Some(days)` runs it after that many days.
#[derive(Clone, Serialize, Deserialize)]
pub struct ThreadRetentionSettings {
    pub archive_after_days: Option<i64>,
    pub remove_after_days: Option<i64>,
}

#[derive(Debug)]
pub enum SettingsApiError {
    Auth(AuthError),
    BadRequest,
    Internal,
}

impl IntoResponse for SettingsApiError {
    fn into_response(self) -> Response {
        match self {
            SettingsApiError::Auth(error) => error.into_response(),
            SettingsApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            SettingsApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for SettingsApiError {
    fn from(error: AuthError) -> Self {
        SettingsApiError::Auth(error)
    }
}

pub async fn get(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<ThreadRetentionSettings>, SettingsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    load(&state.db, owner)
        .await
        .map(Json)
        .map_err(|_| SettingsApiError::Internal)
}

pub async fn update(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<ThreadRetentionSettings>,
) -> Result<Json<ThreadRetentionSettings>, SettingsApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;

    let archive = sanitize(request.archive_after_days)?;
    let remove = sanitize(request.remove_after_days)?;

    state
        .db
        .query_with_params(
            "INSERT INTO thread_retention_settings (owner, archive_after_days, remove_after_days, updated_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(owner) DO UPDATE SET \
                archive_after_days = excluded.archive_after_days, \
                remove_after_days = excluded.remove_after_days, \
                updated_at = excluded.updated_at",
            vec![
                Value::Uuid(owner),
                opt_int(archive),
                opt_int(remove),
                Value::Integer(state.clock.now_unix_millis()),
            ],
        )
        .await
        .map_err(|_| SettingsApiError::Internal)?;

    Ok(Json(ThreadRetentionSettings {
        archive_after_days: archive,
        remove_after_days: remove,
    }))
}

/// Reads a user's effective policy, substituting the defaults when no row
/// exists. A stored row with a NULL day count is a deliberate "disabled".
pub(crate) async fn load(
    db: &ConnectionPool,
    owner: Uuid,
) -> anyhow::Result<ThreadRetentionSettings> {
    let rows = db
        .query_with_params(
            "SELECT archive_after_days, remove_after_days FROM thread_retention_settings WHERE owner = ? LIMIT 1",
            vec![Value::Uuid(owner)],
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(match rows.rows().first() {
        Some(row) => ThreadRetentionSettings {
            archive_after_days: row.get_int("archive_after_days"),
            remove_after_days: row.get_int("remove_after_days"),
        },
        None => ThreadRetentionSettings {
            archive_after_days: Some(DEFAULT_ARCHIVE_DAYS),
            remove_after_days: Some(DEFAULT_REMOVE_DAYS),
        },
    })
}

fn sanitize(value: Option<i64>) -> Result<Option<i64>, SettingsApiError> {
    match value {
        None => Ok(None),
        Some(days) if (1..=MAX_DAYS).contains(&days) => Ok(Some(days)),
        Some(_) => Err(SettingsApiError::BadRequest),
    }
}

fn opt_int(value: Option<i64>) -> Value {
    value.map(Value::Integer).unwrap_or(Value::Null)
}
