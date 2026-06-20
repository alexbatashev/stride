use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::auth::{self, AuthError},
    db::{TriggerKind, automations, email_accounts},
    email::{EmailConnection, ImapService, encryption_secret},
};

#[derive(Serialize)]
pub struct EmailAccountResponse {
    pub id: String,
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub inbox_mailbox: String,
    pub sent_mailbox: String,
    pub drafts_mailbox: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct CreateEmailAccountRequest {
    name: String,
    email: String,
    host: String,
    port: Option<u16>,
    username: String,
    password: String,
    inbox_mailbox: Option<String>,
    sent_mailbox: Option<String>,
    drafts_mailbox: Option<String>,
}

#[derive(Debug)]
pub enum EmailApiError {
    Auth(AuthError),
    BadRequest(String),
    Conflict(String),
    NotFound,
    Internal,
}

impl IntoResponse for EmailApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            EmailApiError::Auth(error) => return error.into_response(),
            EmailApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            EmailApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            EmailApiError::NotFound => (StatusCode::NOT_FOUND, "email account not found".into()),
            EmailApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
            ),
        };
        (status, Json(json!({"error": message}))).into_response()
    }
}

impl From<AuthError> for EmailApiError {
    fn from(error: AuthError) -> Self {
        EmailApiError::Auth(error)
    }
}

pub async fn list(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<EmailAccountResponse>>, EmailApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let rows = email_accounts::select()
        .where_(email_accounts::owner.eq(owner))
        .order_by_asc(email_accounts::name)
        .all(&state.db)
        .await
        .map_err(|_| EmailApiError::Internal)?;
    Ok(Json(
        rows.into_iter()
            .map(|row| EmailAccountResponse {
                id: row.id.to_string(),
                name: row.name,
                email: row.email,
                host: row.host,
                port: row.port,
                username: row.username,
                inbox_mailbox: row.inbox_mailbox,
                sent_mailbox: row.sent_mailbox,
                drafts_mailbox: row.drafts_mailbox,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

pub async fn create(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<CreateEmailAccountRequest>,
) -> Result<(StatusCode, Json<EmailAccountResponse>), EmailApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let name = required(request.name, "account name")?;
    let connection = EmailConnection {
        email: required(request.email, "email address")?,
        host: required(request.host, "IMAP host")?,
        port: request.port.unwrap_or(993),
        username: required(request.username, "username")?,
        password: required(request.password, "password")?,
        inbox_mailbox: optional_default(request.inbox_mailbox, "INBOX"),
        sent_mailbox: optional_default(request.sent_mailbox, "Sent"),
        drafts_mailbox: optional_default(request.drafts_mailbox, "Drafts"),
    };
    if connection.port == 0 {
        return Err(EmailApiError::BadRequest("invalid IMAP port".to_string()));
    }

    let duplicate = email_accounts::select_cols((email_accounts::id,))
        .where_(
            email_accounts::owner
                .eq(owner)
                .and(email_accounts::name.eq(name.as_str())),
        )
        .all(&state.db)
        .await
        .map_err(|_| EmailApiError::Internal)?;
    if !duplicate.is_empty() {
        return Err(EmailApiError::Conflict(
            "an email account with this name already exists".to_string(),
        ));
    }

    let service = ImapService::new(state.db.clone(), &encryption_secret(&state.jwt_secret));
    service
        .test_connection(&connection)
        .await
        .map_err(EmailApiError::BadRequest)?;

    let id = Uuid::now_v7();
    let created_at = now();
    let password_ciphertext = service
        .encrypt_password(id, &connection.password)
        .map_err(|_| EmailApiError::Internal)?;
    email_accounts::insert()
        .id(id)
        .owner(owner)
        .name(name.as_str())
        .email(connection.email.as_str())
        .host(connection.host.as_str())
        .port(i64::from(connection.port))
        .username(connection.username.as_str())
        .password_ciphertext(password_ciphertext.as_str())
        .inbox_mailbox(connection.inbox_mailbox.as_str())
        .sent_mailbox(connection.sent_mailbox.as_str())
        .drafts_mailbox(connection.drafts_mailbox.as_str())
        .created_at(created_at)
        .execute(&state.db)
        .await
        .map_err(|_| EmailApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(EmailAccountResponse {
            id: id.to_string(),
            name,
            email: connection.email,
            host: connection.host,
            port: i64::from(connection.port),
            username: connection.username,
            inbox_mailbox: connection.inbox_mailbox,
            sent_mailbox: connection.sent_mailbox,
            drafts_mailbox: connection.drafts_mailbox,
            created_at,
        }),
    ))
}

pub async fn delete(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, EmailApiError> {
    let owner = auth::authenticated_user(&state, &headers).await?;
    let exists = email_accounts::select_cols((email_accounts::id,))
        .where_(
            email_accounts::id
                .eq(id)
                .and(email_accounts::owner.eq(owner)),
        )
        .all(&state.db)
        .await
        .map_err(|_| EmailApiError::Internal)?;
    if exists.is_empty() {
        return Err(EmailApiError::NotFound);
    }

    let automations = automations::select()
        .where_(automations::owner.eq(owner))
        .all(&state.db)
        .await
        .map_err(|_| EmailApiError::Internal)?;
    let in_use = automations.into_iter().any(|automation| {
        TriggerKind::from_opt(automation.trigger_kind.as_deref()) == TriggerKind::Email
            && automation
                .trigger_config
                .as_deref()
                .and_then(|config| serde_json::from_str::<serde_json::Value>(config).ok())
                .and_then(|config| config.get("account_id")?.as_str().map(str::to_string))
                .is_some_and(|account_id| account_id == id.to_string())
    });
    if in_use {
        return Err(EmailApiError::Conflict(
            "remove automations monitoring this inbox first".to_string(),
        ));
    }

    email_accounts::delete()
        .where_(
            email_accounts::id
                .eq(id)
                .and(email_accounts::owner.eq(owner)),
        )
        .execute(&state.db)
        .await
        .map_err(|_| EmailApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

fn required(value: String, field: &str) -> Result<String, EmailApiError> {
    let value = value.trim().to_string();
    if value.is_empty() || value.contains(['\r', '\n']) {
        Err(EmailApiError::BadRequest(format!("invalid {field}")))
    } else {
        Ok(value)
    }
}

fn optional_default(value: Option<String>, default: &str) -> String {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
