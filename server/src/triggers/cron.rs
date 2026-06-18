use async_trait::async_trait;
use minisql::ConnectionPool;

use super::PolledTrigger;
use crate::cron::Cron;

pub struct CronTrigger(Cron);

impl CronTrigger {
    pub fn parse(schedule: &str) -> Result<Self, String> {
        Cron::parse(schedule).map(CronTrigger)
    }
}

#[async_trait(?Send)]
impl PolledTrigger for CronTrigger {
    async fn due(&self, _db: &ConnectionPool, now: i64, last_run: Option<i64>) -> bool {
        // Minute-level dedup: never fire twice within the same wall-clock minute.
        let fresh = last_run.is_none_or(|last| last / 60 != now / 60);
        fresh && self.0.matches_at(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fires_on_matching_minute_once() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        // 2021-01-01 00:00:00 UTC is a Friday; "* * * * *" matches every minute.
        let trigger = CronTrigger::parse("* * * * *").unwrap();
        let now = 1_609_459_200;
        assert!(trigger.due(&db, now, None).await);
        // Already ran this minute -> not fresh.
        assert!(!trigger.due(&db, now, Some(now)).await);
        // A minute later it is fresh again.
        assert!(trigger.due(&db, now + 60, Some(now)).await);
    }

    #[tokio::test]
    async fn rejects_non_matching_minute() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        // Only minute 30 of every hour.
        let trigger = CronTrigger::parse("30 * * * *").unwrap();
        let now = 1_609_459_200; // minute 0
        assert!(!trigger.due(&db, now, None).await);
    }
}
