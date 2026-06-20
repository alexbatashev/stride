use std::{collections::HashSet, future::Future, sync::Arc, time::Duration};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use friday_agent::tools::email::{
    EmailAccount, EmailDraft, EmailMailbox, EmailMessage, EmailProvider,
};
use futures::TryStreamExt;
use mail_parser::{Address, Message, MessageParser};
use minisql::ConnectionPool;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_native_tls::{TlsConnector, TlsStream, native_tls};
use uuid::Uuid;

use crate::db::email_accounts;

const IMAP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TRIGGER_MESSAGES: usize = 50;
const MAX_MESSAGE_BODY_CHARS: usize = 20_000;

type ImapSession = async_imap::Session<TlsStream<TcpStream>>;

#[derive(Clone)]
pub struct ImapService {
    db: ConnectionPool,
    cipher: CredentialCipher,
}

#[derive(Clone)]
struct CredentialCipher {
    key: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct EmailConnection {
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub inbox_mailbox: String,
    pub sent_mailbox: String,
    pub drafts_mailbox: String,
}

#[derive(Clone, Debug)]
pub struct NewEmailBatch {
    pub messages: Vec<EmailMessage>,
    pub cursor: u32,
}

pub fn encryption_secret(fallback: &str) -> String {
    std::env::var("FRIDAY_EMAIL_ENCRYPTION_KEY").unwrap_or_else(|_| fallback.to_string())
}

impl ImapService {
    pub fn new(db: ConnectionPool, encryption_secret: &str) -> Self {
        Self {
            db,
            cipher: CredentialCipher::new(encryption_secret),
        }
    }

    pub fn provider(&self, owner: Uuid) -> Arc<dyn EmailProvider> {
        Arc::new(UserEmailProvider {
            service: self.clone(),
            owner,
        })
    }

    pub fn encrypt_password(&self, account_id: Uuid, password: &str) -> Result<String, String> {
        self.cipher.encrypt(account_id, password)
    }

    pub async fn test_connection(&self, connection: &EmailConnection) -> Result<(), String> {
        validate_connection(connection)?;
        imap_timeout(async {
            let mut session = connect(connection).await?;
            for mailbox in [
                &connection.inbox_mailbox,
                &connection.sent_mailbox,
                &connection.drafts_mailbox,
            ] {
                session
                    .examine(mailbox)
                    .await
                    .map_err(|error| format!("cannot open mailbox '{mailbox}': {error}"))?;
            }
            session.logout().await.map_err(|error| error.to_string())
        })
        .await
    }

    pub async fn current_inbox_uid(&self, owner: Uuid, account_id: Uuid) -> Result<u32, String> {
        imap_timeout(async {
            let connection = self.load_connection(owner, account_id).await?;
            let mut session = connect(&connection).await?;
            session
                .select(&connection.inbox_mailbox)
                .await
                .map_err(|error| error.to_string())?;
            let uids = session
                .uid_search("ALL")
                .await
                .map_err(|error| error.to_string())?;
            let cursor = uids.into_iter().max().unwrap_or(0);
            session.logout().await.map_err(|error| error.to_string())?;
            Ok(cursor)
        })
        .await
    }

    pub async fn new_inbox_messages(
        &self,
        owner: Uuid,
        account_id: Uuid,
        after_uid: u32,
    ) -> Result<NewEmailBatch, String> {
        imap_timeout(async {
            let connection = self.load_connection(owner, account_id).await?;
            let mut session = connect(&connection).await?;
            session
                .select(&connection.inbox_mailbox)
                .await
                .map_err(|error| error.to_string())?;
            let all_uids = session
                .uid_search("ALL")
                .await
                .map_err(|error| error.to_string())?;
            let mut uids: Vec<u32> = all_uids
                .into_iter()
                .filter(|uid| *uid > after_uid)
                .collect();
            uids.sort_unstable();
            uids.truncate(MAX_TRIGGER_MESSAGES);
            let cursor = uids.last().copied().unwrap_or(after_uid);
            let messages = fetch_uids(&mut session, &connection.inbox_mailbox, &uids).await?;
            session.logout().await.map_err(|error| error.to_string())?;
            Ok(NewEmailBatch { messages, cursor })
        })
        .await
    }

    async fn load_connection(
        &self,
        owner: Uuid,
        account_id: Uuid,
    ) -> Result<EmailConnection, String> {
        let rows = email_accounts::select()
            .where_(
                email_accounts::id
                    .eq(account_id)
                    .and(email_accounts::owner.eq(owner)),
            )
            .all(&self.db)
            .await
            .map_err(|error| error.to_string())?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| "email account not found".to_string())?;
        let port = u16::try_from(row.port).map_err(|_| "invalid IMAP port".to_string())?;
        Ok(EmailConnection {
            email: row.email,
            host: row.host,
            port,
            username: row.username,
            password: self.cipher.decrypt(row.id, &row.password_ciphertext)?,
            inbox_mailbox: row.inbox_mailbox,
            sent_mailbox: row.sent_mailbox,
            drafts_mailbox: row.drafts_mailbox,
        })
    }

    async fn list_accounts(&self, owner: Uuid) -> Result<Vec<EmailAccount>, String> {
        email_accounts::select()
            .where_(email_accounts::owner.eq(owner))
            .order_by_asc(email_accounts::name)
            .all(&self.db)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|row| EmailAccount {
                        id: row.id.to_string(),
                        name: row.name,
                        address: row.email,
                    })
                    .collect()
            })
            .map_err(|error| error.to_string())
    }

    async fn list_messages(
        &self,
        owner: Uuid,
        account_id: Uuid,
        mailbox: EmailMailbox,
        limit: usize,
    ) -> Result<Vec<EmailMessage>, String> {
        imap_timeout(async {
            let connection = self.load_connection(owner, account_id).await?;
            let mailbox_name = mailbox_name(&connection, mailbox);
            let mut session = connect(&connection).await?;
            session
                .select(mailbox_name)
                .await
                .map_err(|error| error.to_string())?;
            let mut uids: Vec<u32> = session
                .uid_search("ALL")
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .collect();
            uids.sort_unstable_by(|a, b| b.cmp(a));
            uids.truncate(limit);
            let mut messages = fetch_uids(&mut session, mailbox_name, &uids).await?;
            messages.sort_unstable_by(|a, b| b.uid.cmp(&a.uid));
            session.logout().await.map_err(|error| error.to_string())?;
            Ok(messages)
        })
        .await
    }

    async fn create_draft(
        &self,
        owner: Uuid,
        account_id: Uuid,
        mailbox: EmailMailbox,
        message_uid: u32,
        body: &str,
    ) -> Result<EmailDraft, String> {
        imap_timeout(async {
            let connection = self.load_connection(owner, account_id).await?;
            let source_mailbox = mailbox_name(&connection, mailbox).to_string();
            let mut session = connect(&connection).await?;
            session
                .select(&source_mailbox)
                .await
                .map_err(|error| error.to_string())?;
            let messages = fetch_raw(&mut session, &[message_uid]).await?;
            let original = messages
                .into_iter()
                .next()
                .ok_or_else(|| "source email not found".to_string())?;
            let parsed = MessageParser::default()
                .parse(&original)
                .ok_or_else(|| "source email could not be parsed".to_string())?;
            let (to, cc) = reply_all_recipients(&parsed, &connection.email);
            if to.is_empty() {
                return Err("source email has no reply recipients".to_string());
            }
            let subject = reply_subject(parsed.subject().unwrap_or(""));
            let raw = build_draft(&connection, &parsed, &to, &cc, &subject, body);
            session
                .append(
                    &connection.drafts_mailbox,
                    Some(r"(\Draft)"),
                    None,
                    raw.as_bytes(),
                )
                .await
                .map_err(|error| format!("failed to save draft: {error}"))?;
            session.logout().await.map_err(|error| error.to_string())?;
            Ok(EmailDraft {
                mailbox: connection.drafts_mailbox,
                in_reply_to_uid: message_uid,
                to,
                cc,
                subject,
            })
        })
        .await
    }
}

struct UserEmailProvider {
    service: ImapService,
    owner: Uuid,
}

#[async_trait(?Send)]
impl EmailProvider for UserEmailProvider {
    async fn accounts(&self) -> Result<Vec<EmailAccount>, String> {
        self.service.list_accounts(self.owner).await
    }

    async fn list(
        &self,
        account_id: &str,
        mailbox: EmailMailbox,
        limit: usize,
    ) -> Result<Vec<EmailMessage>, String> {
        let account_id =
            Uuid::parse_str(account_id).map_err(|_| "invalid email account ID".to_string())?;
        self.service
            .list_messages(self.owner, account_id, mailbox, limit)
            .await
    }

    async fn draft_reply_all(
        &self,
        account_id: &str,
        mailbox: EmailMailbox,
        message_uid: u32,
        body: &str,
    ) -> Result<EmailDraft, String> {
        let account_id =
            Uuid::parse_str(account_id).map_err(|_| "invalid email account ID".to_string())?;
        self.service
            .create_draft(self.owner, account_id, mailbox, message_uid, body)
            .await
    }
}

impl CredentialCipher {
    fn new(secret: &str) -> Self {
        Self {
            key: Sha256::digest(secret.as_bytes()).into(),
        }
    }

    fn encrypt(&self, account_id: Uuid, password: &str) -> Result<String, String> {
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| "invalid key".to_string())?;
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                aes_gcm::aead::Payload {
                    msg: password.as_bytes(),
                    aad: account_id.as_bytes(),
                },
            )
            .map_err(|_| "failed to encrypt password".to_string())?;
        let mut encoded = nonce_bytes.to_vec();
        encoded.extend(ciphertext);
        Ok(BASE64.encode(encoded))
    }

    fn decrypt(&self, account_id: Uuid, encoded: &str) -> Result<String, String> {
        let bytes = BASE64
            .decode(encoded)
            .map_err(|_| "invalid encrypted password".to_string())?;
        let (nonce, ciphertext) = bytes
            .split_at_checked(12)
            .ok_or_else(|| "invalid encrypted password".to_string())?;
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| "invalid key".to_string())?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                aes_gcm::aead::Payload {
                    msg: ciphertext,
                    aad: account_id.as_bytes(),
                },
            )
            .map_err(|_| "failed to decrypt password".to_string())?;
        String::from_utf8(plaintext).map_err(|_| "invalid decrypted password".to_string())
    }
}

async fn imap_timeout<T>(future: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::time::timeout(IMAP_TIMEOUT, future)
        .await
        .map_err(|_| "IMAP operation timed out".to_string())?
}

async fn connect(connection: &EmailConnection) -> Result<ImapSession, String> {
    tokio::time::timeout(IMAP_TIMEOUT, async {
        let tcp = TcpStream::connect((connection.host.as_str(), connection.port))
            .await
            .map_err(|error| format!("IMAP connection failed: {error}"))?;
        let native = native_tls::TlsConnector::builder()
            .build()
            .map_err(|error| format!("TLS setup failed: {error}"))?;
        let tls = TlsConnector::from(native)
            .connect(&connection.host, tcp)
            .await
            .map_err(|error| format!("IMAP TLS connection failed: {error}"))?;
        let mut client = async_imap::Client::new(tls);
        client
            .read_response()
            .await
            .map_err(|error| format!("IMAP greeting failed: {error}"))?
            .ok_or_else(|| "IMAP server closed the connection".to_string())?;
        client
            .login(&connection.username, &connection.password)
            .await
            .map_err(|(error, _)| format!("IMAP authentication failed: {error}"))
    })
    .await
    .map_err(|_| "IMAP connection timed out".to_string())?
}

async fn fetch_uids(
    session: &mut ImapSession,
    mailbox: &str,
    uids: &[u32],
) -> Result<Vec<EmailMessage>, String> {
    let raw = fetch_with_metadata(session, uids).await?;
    raw.into_iter()
        .map(|(uid, seen, bytes)| parse_message(uid, mailbox, seen, &bytes))
        .collect()
}

async fn fetch_raw(session: &mut ImapSession, uids: &[u32]) -> Result<Vec<Vec<u8>>, String> {
    Ok(fetch_with_metadata(session, uids)
        .await?
        .into_iter()
        .map(|(_, _, bytes)| bytes)
        .collect())
}

async fn fetch_with_metadata(
    session: &mut ImapSession,
    uids: &[u32],
) -> Result<Vec<(u32, bool, Vec<u8>)>, String> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let sequence = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let stream = session
        .uid_fetch(sequence, "(UID FLAGS BODY.PEEK[])")
        .await
        .map_err(|error| error.to_string())?;
    let fetches = stream
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| error.to_string())?;
    Ok(fetches
        .into_iter()
        .filter_map(|fetch| {
            let uid = fetch.uid?;
            let seen = fetch
                .flags()
                .any(|flag| matches!(flag, async_imap::types::Flag::Seen));
            fetch.body().map(|body| (uid, seen, body.to_vec()))
        })
        .collect())
}

fn parse_message(
    uid: u32,
    mailbox: &str,
    seen: bool,
    bytes: &[u8],
) -> Result<EmailMessage, String> {
    let message = MessageParser::default()
        .parse(bytes)
        .ok_or_else(|| format!("email UID {uid} could not be parsed"))?;
    let body: String = message
        .body_text(0)
        .map(|body| body.chars().take(MAX_MESSAGE_BODY_CHARS).collect())
        .unwrap_or_default();
    Ok(EmailMessage {
        uid,
        mailbox: mailbox.to_string(),
        message_id: message.message_id().map(str::to_string),
        from: display_addresses(message.from()),
        to: display_addresses(message.to()),
        cc: display_addresses(message.cc()),
        subject: message.subject().unwrap_or("").to_string(),
        date: message.date().map(|date| date.to_rfc3339()),
        body,
        seen,
    })
}

fn display_addresses(addresses: Option<&Address<'_>>) -> Vec<String> {
    addresses
        .into_iter()
        .flat_map(Address::iter)
        .filter_map(|address| {
            address.address().map(|email| match address.name() {
                Some(name) => format!("{name} <{email}>"),
                None => email.to_string(),
            })
        })
        .collect()
}

fn raw_addresses(addresses: Option<&Address<'_>>) -> Vec<String> {
    addresses
        .into_iter()
        .flat_map(Address::iter)
        .filter_map(|address| address.address().map(str::to_string))
        .collect()
}

fn reply_all_recipients(message: &Message<'_>, own_address: &str) -> (Vec<String>, Vec<String>) {
    let own = own_address.to_ascii_lowercase();
    let mut seen = HashSet::new();
    seen.insert(own);
    let mut to = Vec::new();
    let primary = message.reply_to().or_else(|| message.from());
    push_unique(&mut to, &mut seen, raw_addresses(primary));
    if to.is_empty() {
        push_unique(&mut to, &mut seen, raw_addresses(message.to()));
    }
    let mut cc = Vec::new();
    push_unique(&mut cc, &mut seen, raw_addresses(message.to()));
    push_unique(&mut cc, &mut seen, raw_addresses(message.cc()));
    (to, cc)
}

fn push_unique(target: &mut Vec<String>, seen: &mut HashSet<String>, values: Vec<String>) {
    for value in values {
        if seen.insert(value.to_ascii_lowercase()) {
            target.push(value);
        }
    }
}

fn build_draft(
    connection: &EmailConnection,
    original: &Message<'_>,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
) -> String {
    let mut headers = vec![
        format!("From: {}", connection.email),
        format!("To: {}", to.join(", ")),
    ];
    if !cc.is_empty() {
        headers.push(format!("Cc: {}", cc.join(", ")));
    }
    headers.push(format!("Subject: {}", encode_header(subject)));
    headers.push(format!(
        "Date: {}",
        mail_parser::DateTime::from_timestamp(now()).to_rfc822()
    ));
    let message_domain: String = connection
        .host
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
        .collect();
    headers.push(format!(
        "Message-ID: <{}@{}>",
        Uuid::now_v7(),
        if message_domain.is_empty() {
            "friday.invalid"
        } else {
            &message_domain
        }
    ));
    if let Some(message_id) = original.message_id() {
        let message_id = format_message_id(message_id);
        headers.push(format!("In-Reply-To: {message_id}"));
        let mut references = original
            .references()
            .as_text_list()
            .into_iter()
            .flatten()
            .map(|value| format_message_id(value))
            .collect::<Vec<_>>();
        references.push(message_id);
        headers.push(format!("References: {}", references.join(" ")));
    }
    headers.push("MIME-Version: 1.0".to_string());
    headers.push("Content-Type: text/plain; charset=UTF-8".to_string());
    headers.push("Content-Transfer-Encoding: 8bit".to_string());
    format!("{}\r\n\r\n{}", headers.join("\r\n"), crlf(body))
}

fn reply_subject(subject: &str) -> String {
    let subject = safe_header(subject).trim().to_string();
    if subject.to_ascii_lowercase().starts_with("re:") {
        subject
    } else {
        format!("Re: {subject}")
    }
}

fn encode_header(value: &str) -> String {
    let value = safe_header(value);
    if value.is_ascii() {
        value
    } else {
        format!("=?UTF-8?B?{}?=", BASE64.encode(value.as_bytes()))
    }
}

fn safe_header(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn format_message_id(value: &str) -> String {
    let value = safe_header(value);
    let value = value.trim().trim_matches(['<', '>']);
    format!("<{value}>")
}

fn crlf(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

fn mailbox_name(connection: &EmailConnection, mailbox: EmailMailbox) -> &str {
    match mailbox {
        EmailMailbox::Inbox => &connection.inbox_mailbox,
        EmailMailbox::Sent => &connection.sent_mailbox,
    }
}

fn validate_connection(connection: &EmailConnection) -> Result<(), String> {
    for (label, value) in [
        ("email", &connection.email),
        ("host", &connection.host),
        ("username", &connection.username),
        ("password", &connection.password),
        ("inbox mailbox", &connection.inbox_mailbox),
        ("sent mailbox", &connection.sent_mailbox),
        ("drafts mailbox", &connection.drafts_mailbox),
    ] {
        if value.trim().is_empty() || value.contains(['\r', '\n']) {
            return Err(format!("invalid {label}"));
        }
    }
    if !connection.email.contains('@') {
        return Err("invalid email address".to_string());
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_authenticated_and_account_bound() {
        let cipher = CredentialCipher::new("secret");
        let account = Uuid::now_v7();
        let other = Uuid::now_v7();
        let encrypted = cipher.encrypt(account, "password").unwrap();
        assert_ne!(encrypted, "password");
        assert_eq!(cipher.decrypt(account, &encrypted).unwrap(), "password");
        assert!(cipher.decrypt(other, &encrypted).is_err());
    }

    #[test]
    fn reply_all_excludes_own_address_and_deduplicates() {
        let message = MessageParser::default()
            .parse(
                b"From: Sender <sender@example.com>\r\nTo: me@example.com, team@example.com\r\nCc: team@example.com, other@example.com\r\nSubject: Test\r\n\r\nBody",
            )
            .unwrap();
        let (to, cc) = reply_all_recipients(&message, "me@example.com");
        assert_eq!(to, vec!["sender@example.com"]);
        assert_eq!(cc, vec!["team@example.com", "other@example.com"]);
    }

    #[test]
    fn draft_has_thread_headers_but_no_send_capability() {
        let message = MessageParser::default()
            .parse(
                b"Message-ID: <original@example.com>\r\nSubject: Test\r\nFrom: sender@example.com\r\n\r\nBody",
            )
            .unwrap();
        let connection = EmailConnection {
            email: "me@example.com".to_string(),
            host: "imap.example.com".to_string(),
            port: 993,
            username: "me@example.com".to_string(),
            password: "secret".to_string(),
            inbox_mailbox: "INBOX".to_string(),
            sent_mailbox: "Sent".to_string(),
            drafts_mailbox: "Drafts".to_string(),
        };
        let draft = build_draft(
            &connection,
            &message,
            &["sender@example.com".to_string()],
            &[],
            "Re: Test",
            "Reply",
        );
        assert!(draft.contains("In-Reply-To: <original@example.com>"));
        assert!(!draft.to_ascii_lowercase().contains("smtp"));
    }
}
