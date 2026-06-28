//! Pluggable automation triggers. A trigger decides *when* an automation runs.
//! Polled triggers (cron, vfs change) are evaluated by the scheduler loop;
//! pushed triggers (webhook, manual) enter through API handlers that fire the
//! executor directly and therefore implement no trait here.

pub mod cron;
pub mod vfs_change;
pub mod webhook;

use async_trait::async_trait;
use minisql::ConnectionPool;
use uuid::Uuid;

use crate::db::TriggerKind;

/// A trigger the scheduler evaluates on every poll tick.
#[async_trait(?Send)]
pub trait PolledTrigger {
    /// Whether the automation should fire now, given the last run watermark.
    async fn due(&self, db: &ConnectionPool, now: i64, last_run: Option<i64>) -> bool;
}

/// Build the polled trigger for an automation, or `None` for pushed triggers
/// (webhook, manual) which never participate in polling.
pub fn polled(
    kind: TriggerKind,
    schedule: &str,
    trigger_config: Option<&str>,
    owner: Uuid,
) -> Option<Box<dyn PolledTrigger>> {
    match kind {
        TriggerKind::Cron => match cron::CronTrigger::parse(schedule) {
            Ok(trigger) => Some(Box::new(trigger)),
            Err(error) => {
                tracing::warn!(%error, schedule, "skipping automation with invalid schedule");
                None
            }
        },
        TriggerKind::VfsChange => {
            match vfs_change::VfsChangeTrigger::parse(trigger_config, owner) {
                Ok(trigger) => Some(Box::new(trigger)),
                Err(error) => {
                    tracing::warn!(%error, "skipping vfs_change automation with invalid config");
                    None
                }
            }
        }
        TriggerKind::Email | TriggerKind::Gmail | TriggerKind::Webhook | TriggerKind::Manual => {
            None
        }
    }
}
