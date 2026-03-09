use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use friday::grpc::generated::friday::core::rpc::{
    AuthReply, HelloReply, HelloRequest, LoginRequest, LogoutReply, LogoutRequest, RegisterRequest,
    auth_service_server::{AuthService, AuthServiceServer},
    hello_service_server::{HelloService, HelloServiceServer},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use ldap3::{LdapConnAsync, Scope, SearchEntry};
use minisql::{ConnectionPool, Value};
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataMap;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::AppState;
use crate::db::{server_sessions, users};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    sid: String,
    exp: usize,
    iat: usize,
}

#[derive(Clone)]
pub(crate) struct GreeterService {
    pub(crate) state: Arc<AppState>,
}

#[derive(Clone)]
pub(crate) struct AuthServiceImpl {
    pub(crate) state: Arc<AppState>,
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn bearer_token(metadata: &MetadataMap) -> Result<&str, Status> {
    let raw = metadata
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("missing authorization metadata"))?;
    let auth = raw
        .to_str()
        .map_err(|_| Status::unauthenticated("invalid authorization metadata"))?;
    let token = auth
        .strip_prefix("Bearer ")
        .ok_or_else(|| Status::unauthenticated("authorization must be Bearer token"))?;
    if token.is_empty() {
        return Err(Status::unauthenticated("empty bearer token"));
    }
    Ok(token)
}

fn cookie_token(metadata: &MetadataMap) -> Option<String> {
    let raw = metadata.get("cookie")?;
    let cookie_header = raw.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("friday_auth=") {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn request_token(metadata: &MetadataMap) -> Result<String, Status> {
    if let Ok(token) = bearer_token(metadata) {
        return Ok(token.to_string());
    }
    if let Some(token) = cookie_token(metadata) {
        return Ok(token);
    }
    Err(Status::unauthenticated(
        "missing auth token (expected Bearer metadata or friday_auth cookie)",
    ))
}

fn cookie_secure_enabled() -> bool {
    std::env::var("FRIDAY_COOKIE_SECURE")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn auth_cookie(token: &str, expires_at: i64) -> String {
    let now = now_epoch_seconds();
    let max_age = (expires_at - now).max(0);
    let secure = if cookie_secure_enabled() {
        "; Secure"
    } else {
        ""
    };
    format!("friday_auth={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
}

fn expired_auth_cookie() -> String {
    let secure = if cookie_secure_enabled() {
        "; Secure"
    } else {
        ""
    };
    format!("friday_auth=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}")
}

fn hash_password(password: &str) -> Result<String, Status> {
    let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
        .map_err(|_| Status::internal("failed to generate password salt"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| Status::internal("failed to hash password"))?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, password_hash: &str) -> Result<(), Status> {
    let parsed = PasswordHash::new(password_hash)
        .map_err(|_| Status::unauthenticated("invalid credentials"))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| Status::unauthenticated("invalid credentials"))
}

fn ldap_user_filter(template: &str, email: &str) -> String {
    template.replace("{email}", email)
}

async fn ldap_authenticate(state: &AppState, email: &str, password: &str) -> Result<bool, Status> {
    let ldap_cfg = &state.ldap;
    if !ldap_cfg.enabled {
        return Ok(false);
    }
    if ldap_cfg.url.is_empty() || ldap_cfg.user_base_dn.is_empty() {
        return Ok(false);
    }

    let (conn, mut ldap) = LdapConnAsync::new(&ldap_cfg.url)
        .await
        .map_err(|_| Status::internal("failed to connect to ldap server"))?;
    ldap3::drive!(conn);

    if !ldap_cfg.bind_dn.is_empty() {
        ldap.simple_bind(&ldap_cfg.bind_dn, &ldap_cfg.bind_password)
            .await
            .map_err(|_| Status::internal("failed to bind ldap service account"))?
            .success()
            .map_err(|_| Status::internal("failed to bind ldap service account"))?;
    }

    let filter = ldap_user_filter(&ldap_cfg.user_filter, email);
    let (entries, _res) = ldap
        .search(
            &ldap_cfg.user_base_dn,
            Scope::Subtree,
            &filter,
            vec!["dn"],
        )
        .await
        .map_err(|_| Status::internal("failed to query ldap user"))?
        .success()
        .map_err(|_| Status::internal("failed to query ldap user"))?;

    let Some(entry) = entries.into_iter().next() else {
        return Ok(false);
    };
    let user_dn = SearchEntry::construct(entry).dn;
    let is_valid = ldap
        .simple_bind(&user_dn, password)
        .await
        .map_err(|_| Status::internal("failed to bind ldap user"))?
        .success()
        .is_ok();

    let _ = ldap.unbind().await;
    Ok(is_valid)
}

async fn internal_authenticate(
    db: &ConnectionPool,
    email: &str,
    password: &str,
) -> Result<Option<users::Row>, Status> {
    let users_found = users::select()
        .where_(users::email.eq(email))
        .limit(1)
        .all(db)
        .await
        .map_err(|_| Status::internal("failed to query users"))?;

    let Some(user) = users_found.first() else {
        return Ok(None);
    };

    match verify_password(password, &user.password_hash) {
        Ok(()) => Ok(Some(user.clone())),
        Err(_) => Ok(None),
    }
}

async fn get_or_create_local_user_for_ldap(state: &AppState, email: &str) -> Result<users::Row, Status> {
    let users_found = users::select()
        .where_(users::email.eq(email))
        .limit(1)
        .all(&state.db)
        .await
        .map_err(|_| Status::internal("failed to query users"))?;
    if let Some(user) = users_found.first() {
        return Ok(user.clone());
    }

    let user_id = Uuid::new_v4();
    let placeholder_password = Uuid::new_v4().to_string();
    let password_hash = hash_password(&placeholder_password)?;
    let created_at = now_epoch_seconds();
    let insert_result = users::insert()
        .id(user_id)
        .email(email)
        .password_hash(password_hash.as_str())
        .created_at(created_at)
        .execute(&state.db)
        .await;

    match insert_result {
        Ok(_) => {
            let created = users::select()
                .where_(users::id.eq(user_id))
                .limit(1)
                .all(&state.db)
                .await
                .map_err(|_| Status::internal("failed to query users"))?;
            created
                .first()
                .cloned()
                .ok_or_else(|| Status::internal("failed to resolve created user"))
        }
        Err(_) => {
            let users_found = users::select()
                .where_(users::email.eq(email))
                .limit(1)
                .all(&state.db)
                .await
                .map_err(|_| Status::internal("failed to query users"))?;
            users_found
                .first()
                .cloned()
                .ok_or_else(|| Status::internal("failed to create local user"))
        }
    }
}

fn issue_jwt(state: &AppState, user_id: Uuid) -> Result<(String, Uuid, i64), Status> {
    let now = now_epoch_seconds();
    let expires_at = now + 60 * 60 * 24;
    let session_id = Uuid::new_v4();
    let claims = Claims {
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        iat: now as usize,
        exp: expires_at as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| Status::internal("failed to sign jwt"))?;

    Ok((token, session_id, expires_at))
}

fn decode_jwt(state: &AppState, token: &str) -> Result<Claims, Status> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| Status::unauthenticated("invalid token"))?;
    Ok(token_data.claims)
}

async fn validate_active_session(state: &AppState, claims: &Claims) -> Result<(), Status> {
    let sid = Uuid::parse_str(&claims.sid).map_err(|_| Status::unauthenticated("invalid token"))?;
    let rows = server_sessions::select()
        .where_(server_sessions::token_id.eq(sid))
        .limit(1)
        .all(&state.db)
        .await
        .map_err(|_| Status::internal("failed to load session"))?;

    let row = rows
        .first()
        .ok_or_else(|| Status::unauthenticated("unknown session"))?;

    if row.user_id.to_string() != claims.sub {
        return Err(Status::unauthenticated("invalid session user"));
    }
    if row.revoked_at.is_some() {
        return Err(Status::unauthenticated("session revoked"));
    }
    if row.expires_at <= now_epoch_seconds() {
        return Err(Status::unauthenticated("session expired"));
    }

    Ok(())
}

#[tonic::async_trait]
impl HelloService for GreeterService {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let token = request_token(request.metadata())?;
        let claims = decode_jwt(&self.state, &token)?;
        validate_active_session(&self.state, &claims).await?;

        let name = request.into_inner().name;
        Ok(Response::new(HelloReply {
            message: format!("hello, {} (user {})", name, claims.sub),
        }))
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<AuthReply>, Status> {
        let body = request.into_inner();
        let email = body.email.trim().to_lowercase();
        if email.is_empty() || body.password.len() < 8 {
            return Err(Status::invalid_argument(
                "email must be set and password must be at least 8 characters",
            ));
        }

        let existing = users::select()
            .where_(users::email.eq(email.as_str()))
            .limit(1)
            .all(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to query users"))?;
        if !existing.is_empty() {
            return Err(Status::already_exists("user already exists"));
        }

        let user_id = Uuid::new_v4();
        let password_hash = hash_password(&body.password)?;
        users::insert()
            .id(user_id)
            .email(email.as_str())
            .password_hash(password_hash.as_str())
            .created_at(now_epoch_seconds())
            .execute(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to create user"))?;

        let (token, session_id, expires_at) = issue_jwt(&self.state, user_id)?;
        server_sessions::insert()
            .id(Uuid::new_v4())
            .user_id(user_id)
            .token_id(session_id)
            .revoked_at(Option::<i64>::None)
            .created_at(now_epoch_seconds())
            .expires_at(expires_at)
            .execute(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to create session"))?;

        let mut response = Response::new(AuthReply {
            token,
            user_id: user_id.to_string(),
            expires_at,
        });
        let set_cookie = auth_cookie(&response.get_ref().token, expires_at);
        let header = MetadataValue::try_from(set_cookie.as_str())
            .map_err(|_| Status::internal("failed to encode auth cookie header"))?;
        response.metadata_mut().insert("set-cookie", header);
        Ok(response)
    }

    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<AuthReply>, Status> {
        let body = request.into_inner();
        let email = body.email.trim().to_lowercase();
        let user = if let Some(user) = internal_authenticate(&self.state.db, &email, &body.password).await? {
            user
        } else if ldap_authenticate(&self.state, &email, &body.password).await? {
            get_or_create_local_user_for_ldap(&self.state, &email).await?
        } else {
            return Err(Status::unauthenticated("invalid credentials"));
        };

        let (token, session_id, expires_at) = issue_jwt(&self.state, user.id)?;
        server_sessions::insert()
            .id(Uuid::new_v4())
            .user_id(user.id)
            .token_id(session_id)
            .revoked_at(Option::<i64>::None)
            .created_at(now_epoch_seconds())
            .expires_at(expires_at)
            .execute(&self.state.db)
            .await
            .map_err(|_| Status::internal("failed to create session"))?;

        let mut response = Response::new(AuthReply {
            token,
            user_id: user.id.to_string(),
            expires_at,
        });
        let set_cookie = auth_cookie(&response.get_ref().token, expires_at);
        let header = MetadataValue::try_from(set_cookie.as_str())
            .map_err(|_| Status::internal("failed to encode auth cookie header"))?;
        response.metadata_mut().insert("set-cookie", header);
        Ok(response)
    }

    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutReply>, Status> {
        let metadata = request.metadata().clone();
        let body = request.into_inner();
        let token = if body.token.trim().is_empty() {
            request_token(&metadata)?
        } else {
            body.token.trim().to_string()
        };

        let claims = decode_jwt(&self.state, &token)?;
        let sid =
            Uuid::parse_str(&claims.sid).map_err(|_| Status::unauthenticated("invalid token"))?;
        self.state
            .db
            .query_with_params(
                "UPDATE server_sessions SET revoked_at = ? WHERE token_id = ?",
                vec![Value::Integer(now_epoch_seconds()), Value::Uuid(sid)],
            )
            .await
            .map_err(|_| Status::internal("failed to revoke session"))?;

        let mut response = Response::new(LogoutReply { success: true });
        let header = MetadataValue::try_from(expired_auth_cookie().as_str())
            .map_err(|_| Status::internal("failed to encode auth cookie header"))?;
        response.metadata_mut().insert("set-cookie", header);
        Ok(response)
    }
}

pub(crate) fn auth_service(state: Arc<AppState>) -> AuthServiceServer<AuthServiceImpl> {
    AuthServiceServer::new(AuthServiceImpl { state })
}

pub(crate) fn hello_service(state: Arc<AppState>) -> HelloServiceServer<GreeterService> {
    HelloServiceServer::new(GreeterService { state })
}
