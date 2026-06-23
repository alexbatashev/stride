use std::sync::Arc;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    cron::Cron,
    db::{
        AutomationKind, NotifyKind, RunStatus, TriggerKind, automation_runs, automations,
        email_accounts,
    },
    email::{ImapService, encryption_secret},
    scheduler::{FireRequest, TriggerSource},
    triggers::{vfs_change::VfsChangeTrigger, webhook},
};

const WEBHOOK_SECRET_HEADER: &str = "x-stride-webhook-secret";

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
    pub trigger_kind: String,
    pub trigger_config: Option<JsonValue>,
    pub notify_kind: String,
    /// Returned only when a webhook automation is created, never on list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_secret: Option<String>,
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
    #[serde(default)]
    schedule: String,
    kind: String,
    payload: String,
    enabled: Option<bool>,
    trigger_kind: Option<String>,
    trigger_config: Option<JsonValue>,
    notify_kind: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateAutomationRequest {
    enabled: bool,
}

#[derive(Deserialize)]
pub struct WebhookQuery {
    token: Option<String>,
}

#[derive(Debug)]
pub enum AutomationApiError {
    Auth(AuthError),
    BadRequest,
    Unauthorized,
    NotFound,
    Internal,
}

impl IntoResponse for AutomationApiError {
    fn into_response(self) -> Response {
        match self {
            AutomationApiError::Auth(error) => error.into_response(),
            AutomationApiError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            AutomationApiError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
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

fn trigger_from_str(kind: &str) -> Option<TriggerKind> {
    match kind {
        "cron" => Some(TriggerKind::Cron),
        "email" => Some(TriggerKind::Email),
        "webhook" => Some(TriggerKind::Webhook),
        "manual" => Some(TriggerKind::Manual),
        "vfs_change" => Some(TriggerKind::VfsChange),
        _ => None,
    }
}

fn notify_from_str(kind: &str) -> Option<NotifyKind> {
    match kind {
        "none" => Some(NotifyKind::None),
        "telegram" => Some(NotifyKind::Telegram),
        _ => None,
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
                trigger_kind: TriggerKind::from_opt(r.trigger_kind.as_deref())
                    .as_str()
                    .to_string(),
                trigger_config: r
                    .trigger_config
                    .as_deref()
                    .and_then(|config| serde_json::from_str(config).ok()),
                notify_kind: NotifyKind::from_opt(r.notify_kind.as_deref())
                    .as_str()
                    .to_string(),
                webhook_secret: None,
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
    let trigger_kind = trigger_from_str(request.trigger_kind.as_deref().unwrap_or("cron"))
        .ok_or(AutomationApiError::BadRequest)?;
    let notify_kind = notify_from_str(request.notify_kind.as_deref().unwrap_or("none"))
        .ok_or(AutomationApiError::BadRequest)?;
    if name.is_empty() || payload.trim().is_empty() {
        return Err(AutomationApiError::BadRequest);
    }

    // Cron is the only trigger that needs a valid schedule; the rest ignore it.
    let (trigger_config, trigger_cursor) = match trigger_kind {
        TriggerKind::Cron => {
            if Cron::parse(&schedule).is_err() {
                return Err(AutomationApiError::BadRequest);
            }
            (None, None)
        }
        TriggerKind::VfsChange => (
            Some(resolve_vfs_config(&state, owner, request.trigger_config.as_ref()).await?),
            None,
        ),
        TriggerKind::Webhook | TriggerKind::Manual => {
            (request.trigger_config.as_ref().map(|c| c.to_string()), None)
        }
        TriggerKind::Email => {
            let account_id =
                resolve_email_account(&state, owner, request.trigger_config.as_ref()).await?;
            let cursor = ImapService::new(state.db.clone(), &encryption_secret(&state.jwt_secret))
                .current_inbox_uid(owner, account_id)
                .await
                .map_err(|_| AutomationApiError::BadRequest)?;
            (
                Some(serde_json::json!({"account_id": account_id}).to_string()),
                Some(cursor.to_string()),
            )
        }
    };

    // Webhook automations get an opaque secret; vfs_change is baselined so
    // pre-existing files do not fire.
    let webhook_secret = match trigger_kind {
        TriggerKind::Webhook => Some(webhook::generate_secret()),
        _ => None,
    };
    let id = Uuid::now_v7();
    let enabled = request.enabled.unwrap_or(true);
    let created_at = now();
    let last_run = match trigger_kind {
        TriggerKind::VfsChange | TriggerKind::Email => Some(created_at),
        _ => None,
    };

    automations::insert()
        .id(id)
        .owner(owner)
        .name(name.as_str())
        .schedule(schedule.as_str())
        .kind(kind)
        .payload(payload.as_str())
        .enabled(enabled)
        .created_at(created_at)
        .last_run(last_run)
        .trigger_kind(Some(trigger_kind.as_str()))
        .trigger_config(trigger_config.as_deref())
        .webhook_secret(webhook_secret.as_deref())
        .notify_kind(Some(notify_kind.as_str()))
        .trigger_cursor(trigger_cursor.as_deref())
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
            last_run,
            trigger_kind: trigger_kind.as_str().to_string(),
            trigger_config: trigger_config
                .as_deref()
                .and_then(|config| serde_json::from_str(config).ok()),
            notify_kind: notify_kind.as_str().to_string(),
            webhook_secret,
        }),
    ))
}

/// Resolve a vfs_change UI config into the canonical stored form. The UI sends
/// `{"path": "..."}` (empty path = all global files); we pin it to a node id or
/// the owner-wide global watch. A pre-canonical `{node|workspace|global}` config
/// (e.g. from an agent) is accepted as-is.
async fn resolve_vfs_config(
    state: &ServerState,
    owner: Uuid,
    config: Option<&JsonValue>,
) -> Result<String, AutomationApiError> {
    let path = config
        .and_then(|c| c.get("path"))
        .and_then(|p| p.as_str())
        .map(str::trim);

    let canonical = match path {
        Some(path) if !path.is_empty() => {
            let vfs = state.vfs.as_ref().ok_or(AutomationApiError::BadRequest)?;
            let node = vfs
                .resolve_global_node(owner, path)
                .await
                .map_err(|_| AutomationApiError::BadRequest)?;
            format!(r#"{{"node":"{node}"}}"#)
        }
        // Explicit empty path, or a config with no path key, defaults to the
        // owner's whole global area; a pre-canonical config passes through.
        Some(_) | None => match config {
            Some(config) if config.get("path").is_none() => config.to_string(),
            _ => r#"{"global":true}"#.to_string(),
        },
    };

    VfsChangeTrigger::parse(Some(&canonical), owner).map_err(|_| AutomationApiError::BadRequest)?;
    Ok(canonical)
}

async fn resolve_email_account(
    state: &ServerState,
    owner: Uuid,
    config: Option<&JsonValue>,
) -> Result<Uuid, AutomationApiError> {
    let account_id = config
        .and_then(|config| config.get("account_id"))
        .and_then(JsonValue::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(AutomationApiError::BadRequest)?;
    let rows = email_accounts::select_cols((email_accounts::id,))
        .where_(
            email_accounts::id
                .eq(account_id)
                .and(email_accounts::owner.eq(owner)),
        )
        .all(&state.db)
        .await
        .map_err(|_| AutomationApiError::Internal)?;
    if rows.is_empty() {
        return Err(AutomationApiError::BadRequest);
    }
    Ok(account_id)
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

/// Fire an automation on demand. Owner-authenticated; the optional JSON body is
/// passed to the action as the trigger payload.
pub async fn run_now(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<StatusCode, AutomationApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    require_owner(&state, owner, id).await?;

    let payload = parse_payload(&body)?;
    state
        .executor
        .fire(FireRequest {
            automation_id: id,
            payload,
            source: TriggerSource::Manual,
        })
        .map_err(|_| AutomationApiError::Internal)?;
    Ok(StatusCode::ACCEPTED)
}

/// Inbound webhook endpoint. Authenticates with the automation's secret (header
/// `x-stride-webhook-secret` or `?token=`), not a user session.
pub async fn webhook(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<WebhookQuery>,
    body: Bytes,
) -> Result<StatusCode, AutomationApiError> {
    let rows = automations::select_cols((
        automations::trigger_kind,
        automations::webhook_secret,
        automations::enabled,
    ))
    .where_(automations::id.eq(id))
    .all(&state.db)
    .await
    .map_err(|_| AutomationApiError::Internal)?;

    let Some((trigger_kind, secret, enabled)) = rows.into_iter().next() else {
        return Err(AutomationApiError::NotFound);
    };
    if TriggerKind::from_opt(trigger_kind.as_deref()) != TriggerKind::Webhook {
        return Err(AutomationApiError::NotFound);
    }
    let expected = secret.ok_or(AutomationApiError::Unauthorized)?;

    let provided = headers
        .get(WEBHOOK_SECRET_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .or(query.token)
        .unwrap_or_default();
    if !webhook::verify_secret(&expected, &provided) {
        return Err(AutomationApiError::Unauthorized);
    }
    if !enabled {
        return Err(AutomationApiError::NotFound);
    }

    let payload = parse_payload(&body)?;
    state
        .executor
        .fire(FireRequest {
            automation_id: id,
            payload,
            source: TriggerSource::Webhook,
        })
        .map_err(|_| AutomationApiError::Internal)?;
    Ok(StatusCode::ACCEPTED)
}

fn parse_payload(body: &Bytes) -> Result<Option<JsonValue>, AutomationApiError> {
    if body.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(body)
        .map(Some)
        .map_err(|_| AutomationApiError::BadRequest)
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
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        body::Bytes,
        extract::{Path, Query, State},
        http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    };
    use minisql::ConnectionPool;
    use stride_agent::{AgentConfig, ModelRegistry};
    use uuid::Uuid;

    use super::{AutomationApiError, WebhookQuery, webhook};
    use crate::{
        ServerState,
        config::Config,
        db::{
            self, AutomationKind, NotifyKind, RunStatus, TriggerKind, automation_runs, automations,
            users,
        },
        runner::inproc::InProcessAgentPool,
        scheduler::ExecutorHandle,
    };

    fn build_state(db: ConnectionPool, executor: ExecutorHandle) -> Arc<ServerState> {
        let model_config = Arc::new(AgentConfig {
            model_registry: ModelRegistry::new(),
            max_iterations: 1,
        });
        let runner = Arc::new(InProcessAgentPool::new(db.clone(), model_config.clone()));
        Arc::new(ServerState {
            config: Config {
                providers: HashMap::new(),
                models: HashMap::new(),
                server: None,
                tools: None,
                mcp: HashMap::new(),
            },
            db,
            jwt_secret: String::new(),
            runner,
            model_config,
            vfs: None,
            telegram_interactions: Arc::new(std::sync::Mutex::new(
                crate::api::telegram::Interactions::default(),
            )),
            cipher: crate::crypto::SecretCipher::new("test-secret"),
            executor,
        })
    }

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
        // Unset trigger/notify normalize to the defaults.
        assert_eq!(
            TriggerKind::from_opt(rows[0].trigger_kind.as_deref()),
            TriggerKind::Cron
        );
        assert_eq!(
            NotifyKind::from_opt(rows[0].notify_kind.as_deref()),
            NotifyKind::None
        );

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

    #[tokio::test]
    async fn webhook_rejects_bad_secret_and_fires_on_good() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        users::insert()
            .id(owner)
            .username("wh")
            .password_hash("x")
            .execute(&db)
            .await
            .unwrap();

        let id = Uuid::now_v7();
        automations::insert()
            .id(id)
            .owner(owner)
            .name("hook")
            .schedule("")
            .kind(AutomationKind::Python)
            .payload("print(1)")
            .enabled(true)
            .created_at(1)
            .trigger_kind(Some("webhook"))
            .webhook_secret(Some("topsecret"))
            .notify_kind(Some("none"))
            .execute(&db)
            .await
            .unwrap();

        let (handle, mut rx) = ExecutorHandle::channel();
        let state = build_state(db.clone(), handle);

        let secret_header = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                HeaderName::from_static(super::WEBHOOK_SECRET_HEADER),
                HeaderValue::from_static(value),
            );
            headers
        };

        // Wrong secret is rejected and nothing fires.
        let res = webhook(
            State(state.clone()),
            secret_header("nope"),
            Path(id),
            Query(WebhookQuery { token: None }),
            Bytes::new(),
        )
        .await;
        assert!(matches!(res, Err(AutomationApiError::Unauthorized)));
        assert!(rx.try_recv().is_err());

        // Correct secret with a JSON body fires a request carrying the payload.
        let res = webhook(
            State(state.clone()),
            secret_header("topsecret"),
            Path(id),
            Query(WebhookQuery { token: None }),
            Bytes::from_static(b"{\"a\":1}"),
        )
        .await;
        assert_eq!(res.unwrap(), StatusCode::ACCEPTED);
        let fired = rx.try_recv().expect("a fire request");
        assert_eq!(fired.automation_id, id);
        assert!(fired.payload.is_some());
    }

    #[tokio::test]
    async fn vfs_change_config_defaults_to_global_and_requires_vfs_for_paths() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let (handle, _rx) = ExecutorHandle::channel();
        let state = build_state(db, handle);
        let owner = Uuid::now_v7();

        // No config and an empty path both watch the whole global area.
        let global = r#"{"global":true}"#;
        assert_eq!(
            super::resolve_vfs_config(&state, owner, None)
                .await
                .unwrap(),
            global
        );
        let empty = serde_json::json!({ "path": "" });
        assert_eq!(
            super::resolve_vfs_config(&state, owner, Some(&empty))
                .await
                .unwrap(),
            global
        );

        // A real path needs a configured VFS (none in this test) -> rejected.
        let with_path = serde_json::json!({ "path": "reports" });
        assert!(
            super::resolve_vfs_config(&state, owner, Some(&with_path))
                .await
                .is_err()
        );
    }
}
