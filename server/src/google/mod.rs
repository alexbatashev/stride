//! Per-user Google integration: OAuth token lifecycle plus native Gmail,
//! Calendar, and Drive API access.
//!
//! A user links their Google account through [`crate::api::google`]; the access
//! and refresh tokens are stored encrypted. [`GoogleService`] loads them on
//! demand, transparently refreshes an expired access token, and exposes typed
//! Gmail/Calendar/Drive operations the agent tools call. Gmail is read plus
//! draft only — nothing here ever calls `messages.send`.

mod api;
mod mime;

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use minisql::ConnectionPool;
use serde_json::{Value, json};
use stride_agent::{Clock, IdGen};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{crypto::SecretCipher, db::google_connections};

pub use api::CalendarEventInput;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Refresh the access token when it has this many seconds or fewer left, so a
/// call started near expiry does not race the clock.
const REFRESH_SKEW_SECONDS: i64 = 60;

/// Server-wide Google client. Holds the OAuth credentials and the cipher used to
/// seal tokens at rest. Cloneable and cheap to pass into worker threads.
#[derive(Clone)]
pub struct GoogleService {
    db: ConnectionPool,
    cipher: SecretCipher,
    client_id: String,
    client_secret: String,
    clock: Arc<dyn Clock>,
    id_gen: Arc<dyn IdGen>,
}

/// Identity returned by the OIDC `id_token` and the freshly minted tokens.
pub struct LinkedTokens {
    pub google_user_id: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub scope: Option<String>,
}

impl GoogleService {
    pub fn with_clock(
        db: ConnectionPool,
        cipher: SecretCipher,
        client_id: String,
        client_secret: String,
        clock: Arc<dyn Clock>,
        id_gen: Arc<dyn IdGen>,
    ) -> Self {
        Self {
            db,
            cipher,
            client_id,
            client_secret,
            clock,
            id_gen,
        }
    }

    /// Whether `user` has a linked Google account. Used to decide if the native
    /// tools should be offered.
    pub async fn is_connected(&self, user: Uuid) -> bool {
        match google_connections::select_cols((google_connections::id,))
            .where_(google_connections::user_id.eq(user))
            .all(&self.db)
            .await
        {
            Ok(rows) => !rows.is_empty(),
            Err(error) => {
                tracing::warn!(%error, user_id = %user, "failed to check Google connection");
                false
            }
        }
    }

    /// Exchange an authorization `code` for tokens and the account identity.
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<LinkedTokens, String> {
        let body = form_encode(&[
            ("code", code),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ]);
        let value = self.post_token(body).await?;

        let access_token = value
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or("token response missing access_token")?
            .to_string();
        let refresh_token = value
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or(
                "token response missing refresh_token; re-consent with prompt=consent is required",
            )?
            .to_string();
        let expires_at = self.clock.now_unix_secs()
            + value
                .get("expires_in")
                .and_then(Value::as_i64)
                .unwrap_or(3600);
        let scope = value
            .get("scope")
            .and_then(Value::as_str)
            .map(str::to_string);
        let id_token = value
            .get("id_token")
            .and_then(Value::as_str)
            .ok_or("token response missing id_token")?;
        let (google_user_id, email) = identity_from_id_token(id_token)?;

        Ok(LinkedTokens {
            google_user_id,
            email,
            access_token,
            refresh_token,
            expires_at,
            scope,
        })
    }

    /// Persist a freshly linked account, replacing any prior link on either side
    /// of the unique constraints (the user may relink, or the Google account may
    /// move to another user).
    pub async fn store_connection(&self, user: Uuid, tokens: LinkedTokens) -> Result<(), String> {
        let id = self.id_gen.new_uuid_v7();
        let access = self.cipher.encrypt(id, &tokens.access_token)?;
        let refresh = self.cipher.encrypt(id, &tokens.refresh_token)?;

        google_connections::delete()
            .where_(
                google_connections::user_id
                    .eq(user)
                    .or(google_connections::google_user_id.eq(tokens.google_user_id.as_str())),
            )
            .execute(&self.db)
            .await
            .map_err(|error| error.to_string())?;
        google_connections::insert()
            .id(id)
            .user_id(user)
            .google_user_id(tokens.google_user_id.as_str())
            .email(tokens.email.as_str())
            .access_token(access.as_str())
            .refresh_token(refresh.as_str())
            .scope(tokens.scope.as_deref())
            .expires_at(tokens.expires_at)
            .connected_at(self.clock.now_unix_secs())
            .execute(&self.db)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Load `user`'s linked email address, or `None` when not connected.
    pub async fn linked_email(&self, user: Uuid) -> Option<String> {
        google_connections::select_cols((google_connections::email,))
            .where_(google_connections::user_id.eq(user))
            .all(&self.db)
            .await
            .ok()?
            .into_iter()
            .next()
            .map(|(email,)| email)
    }

    /// Return a valid (refreshed if necessary) access token for `user`.
    async fn access_token(&self, user: Uuid) -> Result<String, String> {
        let row = google_connections::select()
            .where_(google_connections::user_id.eq(user))
            .all(&self.db)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
            .ok_or("Google account is not connected")?;

        if row.expires_at > self.clock.now_unix_secs() + REFRESH_SKEW_SECONDS {
            return self.cipher.decrypt(row.id, &row.access_token);
        }

        let refresh_token = self.cipher.decrypt(row.id, &row.refresh_token)?;
        let body = form_encode(&[
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("refresh_token", &refresh_token),
            ("grant_type", "refresh_token"),
        ]);
        let value = self.post_token(body).await?;
        let access_token = value
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or("token refresh response missing access_token")?
            .to_string();
        let expires_at = self.clock.now_unix_secs()
            + value
                .get("expires_in")
                .and_then(Value::as_i64)
                .unwrap_or(3600);
        let sealed = self.cipher.encrypt(row.id, &access_token)?;
        google_connections::update()
            .access_token(sealed.as_str())
            .expires_at(expires_at)
            .where_(google_connections::id.eq(row.id))
            .execute(&self.db)
            .await
            .map_err(|error| error.to_string())?;
        Ok(access_token)
    }

    async fn post_token(&self, body: String) -> Result<Value, String> {
        let req = Request::builder()
            .method("POST")
            .uri(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|error| error.to_string())?;
        let (status, body) = timeout(REQUEST_TIMEOUT, tinynet::send_request(req))
            .await
            .map_err(|_| "Google token request timed out".to_string())?
            .map_err(|error| error.to_string())?;
        let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        if !(200..300).contains(&status) {
            let detail = value
                .get("error_description")
                .or_else(|| value.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("Google token endpoint returned {status}: {detail}"));
        }
        Ok(value)
    }
}

/// Decode the unsigned payload of an OIDC `id_token`. The token came straight
/// from Google's token endpoint over TLS in the authorization-code flow, so the
/// transport authenticates it; we only need the claims, not a second signature
/// check.
fn identity_from_id_token(id_token: &str) -> Result<(String, String), String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    let payload = id_token.split('.').nth(1).ok_or("malformed id_token")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "id_token payload is not valid base64url".to_string())?;
    let claims: Value =
        serde_json::from_slice(&bytes).map_err(|_| "id_token payload is not JSON".to_string())?;
    let sub = claims
        .get("sub")
        .and_then(Value::as_str)
        .ok_or("id_token missing sub")?
        .to_string();
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok((sub, email))
}

/// Percent-encode an `application/x-www-form-urlencoded` body.
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode a value, leaving only the RFC 3986 unreserved characters.
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Tiny helper so other modules can build a JSON error the tools return verbatim.
pub(crate) fn tool_error(message: impl Into<String>) -> Value {
    json!({ "success": false, "error": message.into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    #[test]
    fn percent_encode_matches_rfc3986() {
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_encode("Az0-._~"), "Az0-._~");
    }

    #[test]
    fn form_encode_joins_pairs() {
        let body = form_encode(&[("grant_type", "refresh_token"), ("code", "a/b")]);
        assert_eq!(body, "grant_type=refresh_token&code=a%2Fb");
    }

    #[test]
    fn identity_is_read_from_id_token_payload() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"sub\":\"12345\",\"email\":\"me@example.com\"}");
        let token = format!("{header}.{payload}.signature");
        let (sub, email) = identity_from_id_token(&token).unwrap();
        assert_eq!(sub, "12345");
        assert_eq!(email, "me@example.com");
    }
}
