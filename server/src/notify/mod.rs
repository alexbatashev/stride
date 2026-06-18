//! Pluggable result notification. The full run output is always stored in
//! `automation_runs`; a notifier optionally pushes it somewhere else (e.g.
//! Telegram). Notification failures never fail the run.

pub mod telegram;

use async_trait::async_trait;
use minisql::ConnectionPool;
use uuid::Uuid;

use crate::db::{NotifyKind, RunStatus};

pub struct RunResult<'a> {
    pub name: &'a str,
    pub status: RunStatus,
    pub output: &'a str,
}

#[async_trait(?Send)]
pub trait Notifier {
    async fn notify(&self, result: &RunResult<'_>) -> Result<(), String>;
}

/// No-op notifier: the conversation is already stored, nothing else to do.
pub struct NoneNotifier;

#[async_trait(?Send)]
impl Notifier for NoneNotifier {
    async fn notify(&self, _result: &RunResult<'_>) -> Result<(), String> {
        Ok(())
    }
}

/// Build the notifier for an automation. Telegram falls back to no-op when no
/// bot token is configured; a missing per-user connection is handled at
/// delivery time so storage-only still works.
pub fn build(
    kind: NotifyKind,
    owner: Uuid,
    db: &ConnectionPool,
    bot_token: Option<&str>,
) -> Box<dyn Notifier> {
    match kind {
        NotifyKind::Telegram => match bot_token.filter(|token| !token.is_empty()) {
            Some(token) => Box::new(telegram::TelegramNotifier {
                db: db.clone(),
                user_id: owner,
                bot_token: token.to_string(),
            }),
            None => Box::new(NoneNotifier),
        },
        NotifyKind::None => Box::new(NoneNotifier),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn telegram_without_token_falls_back_to_none() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();

        // No token -> NoneNotifier, delivery is a no-op.
        let notifier = build(NotifyKind::Telegram, owner, &db, None);
        let result = RunResult {
            name: "job",
            status: RunStatus::Success,
            output: "done",
        };
        assert!(notifier.notify(&result).await.is_ok());
    }

    #[tokio::test]
    async fn telegram_without_connection_does_not_error() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();

        // Token present but the user never connected Telegram: storage-only.
        let notifier = build(NotifyKind::Telegram, owner, &db, Some("token"));
        let result = RunResult {
            name: "job",
            status: RunStatus::Success,
            output: "done",
        };
        assert!(notifier.notify(&result).await.is_ok());
    }
}
