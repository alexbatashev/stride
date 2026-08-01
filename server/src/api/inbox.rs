use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::{InboxStatus, inbox_items},
    inbox::{self, ApprovalContext, InboxAction, Outcome},
};

#[derive(Serialize)]
pub struct InboxItemResponse {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub rationale: String,
    pub status: String,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
    pub thread_id: Option<String>,
    /// Kind-specific fields the UI renders as a preview of what would happen.
    pub details: JsonValue,
    pub outcome: Option<String>,
    pub outcome_href: Option<String>,
}

#[derive(Serialize)]
pub struct InboxListResponse {
    pub items: Vec<InboxItemResponse>,
    pub pending: u64,
}

#[derive(Debug)]
pub enum InboxApiError {
    Auth(AuthError),
    BadRequest(String),
    NotFound,
    Conflict(String),
    Internal,
}

impl IntoResponse for InboxApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            InboxApiError::Auth(error) => return error.into_response(),
            InboxApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            InboxApiError::NotFound => (StatusCode::NOT_FOUND, "inbox item not found".to_string()),
            InboxApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            InboxApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".to_string(),
            ),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<AuthError> for InboxApiError {
    fn from(error: AuthError) -> Self {
        InboxApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<InboxListResponse>, InboxApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = inbox_items::select()
        .where_(inbox_items::owner.eq(owner))
        .order_by_desc(inbox_items::id)
        .all(&state.db)
        .await
        .map_err(|_| InboxApiError::Internal)?;

    let items: Vec<InboxItemResponse> = rows
        .into_iter()
        .map(|row| {
            let action = serde_json::from_str::<InboxAction>(&row.payload).ok();
            let outcome = row
                .outcome
                .as_deref()
                .and_then(|outcome| serde_json::from_str::<Outcome>(outcome).ok());
            InboxItemResponse {
                id: row.id.to_string(),
                kind: row.kind.as_str().to_string(),
                title: row.title,
                rationale: row.rationale,
                status: row.status.as_str().to_string(),
                created_at: row.created_at,
                resolved_at: row.resolved_at,
                thread_id: row.thread_id.map(|id| id.to_string()),
                details: action
                    .as_ref()
                    .map(action_details)
                    .unwrap_or(JsonValue::Null),
                outcome: outcome.as_ref().map(|outcome| outcome.summary.clone()),
                outcome_href: outcome.and_then(|outcome| outcome.href),
            }
        })
        .collect();

    let pending = items
        .iter()
        .filter(|item| item.status == InboxStatus::Pending.as_str())
        .count() as u64;
    Ok(Json(InboxListResponse { items, pending }))
}

/// A compact, display-only summary of what the action would do. Long bodies are
/// truncated: the inbox is a review surface, not an editor.
fn action_details(action: &InboxAction) -> JsonValue {
    const PREVIEW_LEN: usize = 600;
    let preview = |value: &str| -> String {
        let trimmed = value.trim();
        match trimmed.char_indices().nth(PREVIEW_LEN) {
            Some((index, _)) => format!("{}…", &trimmed[..index]),
            None => trimmed.to_string(),
        }
    };

    match action {
        InboxAction::FollowUp(follow_up) => json!({
            "message": preview(&follow_up.message),
            "project": follow_up.project,
        }),
        InboxAction::Automation(automation) => json!({
            "name": automation.name,
            "schedule": automation.schedule,
            "automation_kind": automation.kind,
            "trigger": automation.trigger,
            "notify": automation.notify,
            "task": preview(&automation.task),
        }),
        InboxAction::Skill(skill) => json!({
            "name": skill.name,
            "description": skill.description,
            "content": preview(&skill.content),
        }),
    }
}

pub async fn approve(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<InboxItemResponse>, InboxApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let row = pending_item(&state, owner, id).await?;
    let action = serde_json::from_str::<InboxAction>(&row.payload)
        .map_err(|_| InboxApiError::BadRequest("suggestion payload is unreadable".to_string()))?;

    let ctx = ApprovalContext {
        db: &state.db,
        clock: state.clock.as_ref(),
        id_gen: state.id_gen.as_ref(),
        skills: state.skills.as_ref(),
        runner: state.runner.as_ref(),
    };
    let resolved_at = state.clock.now_unix_secs();

    match inbox::execute(&ctx, owner, &action).await {
        Ok(outcome) => {
            resolve(&state, id, InboxStatus::Approved, resolved_at, &outcome).await?;
            inbox::publish_pending_count(&state.db, state.id_gen.as_ref(), owner).await;
            Ok(Json(item_response(
                row,
                &action,
                InboxStatus::Approved,
                resolved_at,
                outcome,
            )))
        }
        Err(error) => {
            // The action failed; record why so the row stops looking pending and
            // the user sees the reason instead of a silent no-op.
            let outcome = Outcome {
                summary: error.clone(),
                href: None,
            };
            resolve(&state, id, InboxStatus::Failed, resolved_at, &outcome).await?;
            inbox::publish_pending_count(&state.db, state.id_gen.as_ref(), owner).await;
            Err(InboxApiError::Conflict(error))
        }
    }
}

pub async fn decline(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<InboxItemResponse>, InboxApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let row = pending_item(&state, owner, id).await?;
    let action = serde_json::from_str::<InboxAction>(&row.payload).ok();
    let resolved_at = state.clock.now_unix_secs();
    let outcome = Outcome {
        summary: "Declined".to_string(),
        href: None,
    };

    resolve(&state, id, InboxStatus::Declined, resolved_at, &outcome).await?;
    inbox::publish_pending_count(&state.db, state.id_gen.as_ref(), owner).await;

    let details = action
        .as_ref()
        .map(action_details)
        .unwrap_or(JsonValue::Null);
    Ok(Json(InboxItemResponse {
        details,
        ..item_response_parts(row, InboxStatus::Declined, resolved_at, outcome)
    }))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, InboxApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = inbox_items::select_cols((inbox_items::id,))
        .where_(inbox_items::id.eq(id).and(inbox_items::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| InboxApiError::Internal)?;
    if rows.is_empty() {
        return Err(InboxApiError::NotFound);
    }

    inbox_items::delete()
        .where_(inbox_items::id.eq(id))
        .execute(&state.db)
        .await
        .map_err(|_| InboxApiError::Internal)?;
    inbox::publish_pending_count(&state.db, state.id_gen.as_ref(), owner).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn pending_item(
    state: &ServerState,
    owner: Uuid,
    id: Uuid,
) -> Result<inbox_items::Row, InboxApiError> {
    let rows = inbox_items::select()
        .where_(inbox_items::id.eq(id).and(inbox_items::owner.eq(owner)))
        .all(&state.db)
        .await
        .map_err(|_| InboxApiError::Internal)?;
    let row = rows.into_iter().next().ok_or(InboxApiError::NotFound)?;
    if row.status != InboxStatus::Pending {
        return Err(InboxApiError::Conflict(format!(
            "suggestion was already {}",
            row.status.as_str()
        )));
    }
    Ok(row)
}

async fn resolve(
    state: &ServerState,
    id: Uuid,
    status: InboxStatus,
    resolved_at: i64,
    outcome: &Outcome,
) -> Result<(), InboxApiError> {
    let encoded = serde_json::to_string(outcome).map_err(|_| InboxApiError::Internal)?;
    inbox_items::update()
        .status(status)
        .resolved_at(Some(resolved_at))
        .outcome(Some(encoded.as_str()))
        .where_(inbox_items::id.eq(id))
        .execute(&state.db)
        .await
        .map_err(|_| InboxApiError::Internal)?;
    Ok(())
}

fn item_response(
    row: inbox_items::Row,
    action: &InboxAction,
    status: InboxStatus,
    resolved_at: i64,
    outcome: Outcome,
) -> InboxItemResponse {
    let details = action_details(action);
    InboxItemResponse {
        details,
        ..item_response_parts(row, status, resolved_at, outcome)
    }
}

fn item_response_parts(
    row: inbox_items::Row,
    status: InboxStatus,
    resolved_at: i64,
    outcome: Outcome,
) -> InboxItemResponse {
    InboxItemResponse {
        id: row.id.to_string(),
        kind: row.kind.as_str().to_string(),
        title: row.title,
        rationale: row.rationale,
        status: status.as_str().to_string(),
        created_at: row.created_at,
        resolved_at: Some(resolved_at),
        thread_id: row.thread_id.map(|id| id.to_string()),
        details: JsonValue::Null,
        outcome: Some(outcome.summary),
        outcome_href: outcome.href,
    }
}
