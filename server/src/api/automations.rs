use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    cron::Cron,
    db::{AutomationKind, RunStatus, automation_runs, automations},
};

#[derive(Serialize)]
pub struct AutomationResponse {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub kind: String,
    pub payload: String,
    pub enabled: bool,
    pub created_at: i64,
    pub last_run: Option<i64>,
}

#[derive(Serialize)]
pub struct RunResponse {
    pub id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub output: String,
}

#[derive(Deserialize)]
pub struct CreateAutomationRequest {
    name: String,
    schedule: String,
    kind: String,
    payload: String,
    enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateAutomationRequest {
    enabled: bool,
}

#[derive(Debug)]
pub enum AutomationApiError {
    Auth(AuthError),
    BadRequest,
    NotFound,
    Internal,
}

impl IntoResponse for AutomationApiError {
    fn into_response(self) -> Response {
        match self {
            AutomationApiError::Auth(error) => error.into_response(),
            AutomationApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            AutomationApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            AutomationApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

impl From<AuthError> for AutomationApiError {
    fn from(error: AuthError) -> Self {
        AutomationApiError::Auth(error)
    }
}

fn kind_to_str(kind: AutomationKind) -> &'static str {
    match kind {
        AutomationKind::Python => "python",
        AutomationKind::Agent => "agent",
    }
}

fn kind_from_str(kind: &str) -> Option<AutomationKind> {
    match kind {
        "python" => Some(AutomationKind::Python),
        "agent" => Some(AutomationKind::Agent),
        _ => None,
    }
}

fn status_to_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Success => "success",
        RunStatus::Failed => "failed",
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AutomationResponse>>, AutomationApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = automations::select()
        .where_(automations::owner.eq(owner))
        .order_by_desc(automations::id)
        .all(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AutomationResponse {
                id: r.id.to_string(),
                name: r.name,
                schedule: r.schedule,
                kind: kind_to_str(r.kind).to_string(),
                payload: r.payload,
                enabled: r.enabled,
                created_at: r.created_at,
                last_run: r.last_run,
            })
            .collect(),
    ))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<CreateAutomationRequest>,
) -> Result<(StatusCode, Json<AutomationResponse>), AutomationApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let name = request.name.trim().to_string();
    let schedule = request.schedule.trim().to_string();
    let payload = request.payload;
    let kind = kind_from_str(&request.kind).ok_or(AutomationApiError::BadRequest)?;
    if name.is_empty() || payload.trim().is_empty() || Cron::parse(&schedule).is_err() {
        return Err(AutomationApiError::BadRequest);
    }

    let id = Uuid::now_v7();
    let enabled = request.enabled.unwrap_or(true);
    let created_at = now();
    automations::insert()
        .id(id)
        .owner(owner)
        .name(name.as_str())
        .schedule(schedule.as_str())
        .kind(kind)
        .payload(payload.as_str())
        .enabled(enabled)
        .created_at(created_at)
        .execute(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(AutomationResponse {
            id: id.to_string(),
            name,
            schedule,
            kind: kind_to_str(kind).to_string(),
            payload,
            enabled,
            created_at,
            last_run: None,
        }),
    ))
}

pub async fn update(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateAutomationRequest>,
) -> Result<StatusCode, AutomationApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_owner(&state, owner, id).await?;

    automations::update()
        .enabled(request.enabled)
        .where_(automations::id.eq(id))
        .execute(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AutomationApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_owner(&state, owner, id).await?;

    automation_runs::delete()
        .where_(automation_runs::automation_id.eq(id))
        .execute(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;
    automations::delete()
        .where_(automations::id.eq(id))
        .execute(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn runs(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<RunResponse>>, AutomationApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_owner(&state, owner, id).await?;

    let rows = automation_runs::select()
        .where_(automation_runs::automation_id.eq(id))
        .order_by_desc(automation_runs::id)
        .all(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|r| RunResponse {
                id: r.id.to_string(),
                started_at: r.started_at,
                finished_at: r.finished_at,
                status: status_to_str(r.status).to_string(),
                output: r.output,
            })
            .collect(),
    ))
}

async fn require_owner(
    state: &ServerState,
    owner: Uuid,
    id: Uuid,
) -> Result<(), AutomationApiError> {
    let rows = automations::select_cols((automations::id,))
        .where_(automations::id.eq(id).and(automations::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;
    if rows.is_empty() {
        Err(AutomationApiError::NotFound)
    } else {
        Ok(())
    }
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use minisql::ConnectionPool;
    use uuid::Uuid;

    use crate::db::{self, AutomationKind, RunStatus, automation_runs, automations, users};

    #[tokio::test]
    async fn automation_and_run_roundtrip() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username("bob")
            .password_hash("x")
            .execute(&db)
            .await
            .unwrap();

        let id = Uuid::now_v7();
        automations::insert()
            .id(id)
            .owner(owner)
            .name("daily")
            .schedule("0 9 * * *")
            .kind(AutomationKind::Agent)
            .payload("summarize news")
            .enabled(true)
            .created_at(1)
            .execute(&db)
            .await
            .unwrap();

        let rows = automations::select()
            .where_(automations::owner.eq(owner))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, AutomationKind::Agent);
        assert!(rows[0].enabled);
        assert_eq!(rows[0].last_run, None);

        let run = Uuid::now_v7();
        automation_runs::insert()
            .id(run)
            .automation_id(id)
            .started_at(2)
            .status(RunStatus::Success)
            .output("done")
            .execute(&db)
            .await
            .unwrap();

        let runs = automation_runs::select()
            .where_(automation_runs::automation_id.eq(id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Success);
        assert_eq!(runs[0].finished_at, None);
        assert_eq!(runs[0].output, "done");
    }
}
