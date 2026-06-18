use async_trait::async_trait;
use minisql::ConnectionPool;
use uuid::Uuid;

use super::{Notifier, RunResult};
use crate::db::RunStatus;
use crate::tools::telegram::{connected_chat, send_message};

pub struct TelegramNotifier {
    pub db: ConnectionPool,
    pub user_id: Uuid,
    pub bot_token: String,
}

#[async_trait(?Send)]
impl Notifier for TelegramNotifier {
    async fn notify(&self, result: &RunResult<'_>) -> Result<(), String> {
        let chat_id = match connected_chat(&self.db, self.user_id).await {
            Ok(Some(chat_id)) => chat_id,
            Ok(None) => {
                tracing::info!(user_id = %self.user_id, "telegram not connected; stored only");
                return Ok(());
            }
            Err(error) => return Err(error),
        };

        let icon = match result.status {
            RunStatus::Success => "✅",
            RunStatus::Failed => "❌",
            RunStatus::Running => "⏳",
        };
        let body: String = result.output.chars().take(3500).collect();
        let text = format!("{icon} {}\n\n{body}", result.name);

        if send_message(&self.bot_token, chat_id, &text)
            .await
            .is_none()
        {
            return Err("telegram sendMessage failed".to_string());
        }
        Ok(())
    }
}
