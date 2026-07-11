use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserEvent {
    pub id: Uuid,
    pub kind: UserEventKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserEventKind {
    ThreadCreated {
        thread_id: Uuid,
        title: String,
        project_id: Option<Uuid>,
    },
    ThreadRenamed {
        thread_id: Uuid,
        title: String,
    },
    ThreadArchived {
        thread_id: Uuid,
    },
    ThreadRestored {
        thread_id: Uuid,
    },
    ThreadDeleted {
        thread_id: Uuid,
    },
    ThreadRunStatus {
        thread_id: Uuid,
        running: bool,
    },
    Notification {
        notification_id: Uuid,
        title: String,
        message: String,
        thread_id: Option<Uuid>,
    },
    Resync,
}

pub fn topic(owner: Uuid) -> String {
    format!("user-events:{owner}")
}

pub fn publish(owner: Uuid, id: Uuid, kind: UserEventKind) {
    let _ = pubsub::topic::<UserEvent>(&topic(owner)).publish(&UserEvent { id, kind });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn user_topics_are_isolated() {
        let owner = Uuid::now_v7();
        let other = Uuid::now_v7();
        let mut events = pubsub::topic::<UserEvent>(&topic(owner)).subscribe();
        let mut other_events = pubsub::topic::<UserEvent>(&topic(other)).subscribe();
        let event_id = Uuid::now_v7();

        publish(
            owner,
            event_id,
            UserEventKind::ThreadRunStatus {
                thread_id: Uuid::now_v7(),
                running: true,
            },
        );

        assert_eq!(events.recv().await.unwrap().id, event_id);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), other_events.recv())
                .await
                .is_err()
        );
    }
}
