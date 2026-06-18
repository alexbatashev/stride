use async_trait::async_trait;
use minisql::{ConnectionPool, Value};
use serde::Deserialize;
use uuid::Uuid;

use super::PolledTrigger;

/// Fires when a watched VFS node or workspace gains a new object version after
/// the last run watermark. Configured via `trigger_config` JSON, e.g.
/// `{"workspace": "<uuid>"}` or `{"node": "<uuid>"}`.
pub struct VfsChangeTrigger {
    workspace: Option<Uuid>,
    node: Option<Uuid>,
}

#[derive(Deserialize)]
struct VfsChangeConfig {
    workspace: Option<Uuid>,
    node: Option<Uuid>,
}

impl VfsChangeTrigger {
    pub fn parse(config: Option<&str>) -> Result<Self, String> {
        let raw = config.ok_or("vfs_change trigger requires trigger_config")?;
        let parsed: VfsChangeConfig = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if parsed.workspace.is_none() && parsed.node.is_none() {
            return Err("vfs_change trigger_config needs 'workspace' or 'node'".to_string());
        }
        Ok(VfsChangeTrigger {
            workspace: parsed.workspace,
            node: parsed.node,
        })
    }

    async fn changed_since(&self, db: &ConnectionPool, watermark: i64) -> bool {
        if let Some(node) = self.node
            && any_row(
                db,
                "SELECT id FROM vfs_objects WHERE node = ? AND created_at > ? LIMIT 1",
                vec![Value::Uuid(node), Value::Integer(watermark)],
            )
            .await
        {
            return true;
        }
        if let Some(workspace) = self.workspace
            && any_row(
                db,
                "SELECT o.id FROM vfs_objects o \
                 INNER JOIN vfs_nodes n ON n.id = o.node \
                 WHERE n.parent_workspace = ? AND o.created_at > ? LIMIT 1",
                vec![Value::Uuid(workspace), Value::Integer(watermark)],
            )
            .await
        {
            return true;
        }
        false
    }
}

#[async_trait(?Send)]
impl PolledTrigger for VfsChangeTrigger {
    async fn due(&self, db: &ConnectionPool, _now: i64, last_run: Option<i64>) -> bool {
        // last_run is the watermark, seeded at creation so pre-existing files do
        // not fire. Without it we cannot tell new from old, so never fire.
        let Some(watermark) = last_run else {
            return false;
        };
        self.changed_since(db, watermark).await
    }
}

async fn any_row(db: &ConnectionPool, sql: &str, params: Vec<Value>) -> bool {
    match db.query_with_params(sql, params).await {
        Ok(result) => !result.rows().is_empty(),
        Err(error) => {
            tracing::warn!(%error, "vfs_change query failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, ObjectKind, vfs_nodes, vfs_objects, vfs_workspaces};

    #[tokio::test]
    async fn fires_on_new_object_in_workspace() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        db::users::insert()
            .id(owner)
            .username("watcher")
            .password_hash("x")
            .execute(&db)
            .await
            .unwrap();

        let workspace = Uuid::now_v7();
        vfs_workspaces::insert()
            .id(workspace)
            .execute(&db)
            .await
            .unwrap();
        let node = Uuid::now_v7();
        vfs_nodes::insert()
            .id(node)
            .name("file.txt")
            .kind(ObjectKind::File)
            .parent_workspace(Some(workspace))
            .owner(owner)
            .created_at(100)
            .execute(&db)
            .await
            .unwrap();
        vfs_objects::insert()
            .id(Uuid::now_v7())
            .version(1)
            .location("disk/1")
            .created_at(150)
            .node(node)
            .size(10)
            .execute(&db)
            .await
            .unwrap();

        let config = format!(r#"{{"workspace":"{workspace}"}}"#);
        let trigger = VfsChangeTrigger::parse(Some(&config)).unwrap();

        // Watermark before the object -> fires.
        assert!(trigger.due(&db, 200, Some(100)).await);
        // Watermark after the object -> no change.
        assert!(!trigger.due(&db, 200, Some(150)).await);
        // No watermark yet -> baseline, never fires.
        assert!(!trigger.due(&db, 200, None).await);
    }

    #[test]
    fn rejects_empty_config() {
        assert!(VfsChangeTrigger::parse(None).is_err());
        assert!(VfsChangeTrigger::parse(Some("{}")).is_err());
    }
}
