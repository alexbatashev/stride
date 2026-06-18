use async_trait::async_trait;
use minisql::{ConnectionPool, Value};
use serde::Deserialize;
use uuid::Uuid;

use super::PolledTrigger;

/// Fires when a watched part of the VFS gains a new object version after the
/// last run watermark. Configured via `trigger_config` JSON:
/// - `{"global": true}` watches the owner's entire global file area
/// - `{"node": "<uuid>"}` watches a node and its descendants (a file or folder)
/// - `{"workspace": "<uuid>"}` watches a thread/project workspace
pub struct VfsChangeTrigger {
    owner: Uuid,
    target: Target,
}

enum Target {
    Global,
    Node(Uuid),
    Workspace(Uuid),
}

#[derive(Deserialize)]
struct VfsChangeConfig {
    #[serde(default)]
    global: bool,
    node: Option<Uuid>,
    workspace: Option<Uuid>,
}

impl VfsChangeTrigger {
    pub fn parse(config: Option<&str>, owner: Uuid) -> Result<Self, String> {
        let raw = config.ok_or("vfs_change trigger requires trigger_config")?;
        let parsed: VfsChangeConfig = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        let target = if let Some(node) = parsed.node {
            Target::Node(node)
        } else if let Some(workspace) = parsed.workspace {
            Target::Workspace(workspace)
        } else if parsed.global {
            Target::Global
        } else {
            return Err("vfs_change trigger_config needs 'global', 'node', or 'workspace'".into());
        };
        Ok(VfsChangeTrigger { owner, target })
    }

    async fn changed_since(&self, db: &ConnectionPool, watermark: i64) -> bool {
        match &self.target {
            Target::Global => {
                any_row(
                    db,
                    "SELECT o.id FROM vfs_objects o \
                     INNER JOIN vfs_nodes n ON n.id = o.node \
                     WHERE n.owner = ? AND n.parent_workspace IS NULL AND o.created_at > ? LIMIT 1",
                    vec![Value::Uuid(self.owner), Value::Integer(watermark)],
                )
                .await
            }
            Target::Node(node) => {
                // Walk the node subtree so watching a folder catches changes to
                // any file beneath it; a file node matches just its own objects.
                any_row(
                    db,
                    "WITH RECURSIVE sub(id) AS ( \
                       SELECT id FROM vfs_nodes WHERE id = ? \
                       UNION ALL \
                       SELECT n.id FROM vfs_nodes n INNER JOIN sub ON n.parent_node = sub.id \
                     ) \
                     SELECT o.id FROM vfs_objects o INNER JOIN sub ON o.node = sub.id \
                     WHERE o.created_at > ? LIMIT 1",
                    vec![Value::Uuid(*node), Value::Integer(watermark)],
                )
                .await
            }
            Target::Workspace(workspace) => {
                any_row(
                    db,
                    "SELECT o.id FROM vfs_objects o \
                     INNER JOIN vfs_nodes n ON n.id = o.node \
                     WHERE n.parent_workspace = ? AND o.created_at > ? LIMIT 1",
                    vec![Value::Uuid(*workspace), Value::Integer(watermark)],
                )
                .await
            }
        }
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

    async fn seed_user(db: &ConnectionPool) -> Uuid {
        let owner = Uuid::now_v7();
        db::users::insert()
            .id(owner)
            .username(format!("u{}", owner.as_simple()).as_str())
            .password_hash("x")
            .execute(db)
            .await
            .unwrap();
        owner
    }

    async fn add_object(db: &ConnectionPool, node: Uuid, created_at: i64) {
        vfs_objects::insert()
            .id(Uuid::now_v7())
            .version(1)
            .location(format!("disk/{}", Uuid::now_v7().as_simple()).as_str())
            .created_at(created_at)
            .node(node)
            .size(10)
            .execute(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn fires_on_new_object_in_workspace() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = seed_user(&db).await;

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
        add_object(&db, node, 150).await;

        let config = format!(r#"{{"workspace":"{workspace}"}}"#);
        let trigger = VfsChangeTrigger::parse(Some(&config), owner).unwrap();

        assert!(trigger.due(&db, 200, Some(100)).await);
        assert!(!trigger.due(&db, 200, Some(150)).await);
        assert!(!trigger.due(&db, 200, None).await);
    }

    #[tokio::test]
    async fn global_watch_fires_on_any_owned_file() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = seed_user(&db).await;
        let other = seed_user(&db).await;

        // A global node owned by `owner` (parent_workspace NULL).
        let node = Uuid::now_v7();
        vfs_nodes::insert()
            .id(node)
            .name("notes.md")
            .kind(ObjectKind::File)
            .owner(owner)
            .created_at(100)
            .execute(&db)
            .await
            .unwrap();
        add_object(&db, node, 150).await;

        let trigger = VfsChangeTrigger::parse(Some(r#"{"global":true}"#), owner).unwrap();
        assert!(trigger.due(&db, 200, Some(100)).await);
        assert!(!trigger.due(&db, 200, Some(150)).await);

        // Another user's global watch must not see this file.
        let other_trigger = VfsChangeTrigger::parse(Some(r#"{"global":true}"#), other).unwrap();
        assert!(!other_trigger.due(&db, 200, Some(100)).await);
    }

    #[tokio::test]
    async fn node_watch_covers_folder_subtree() {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = seed_user(&db).await;

        // folder/ -> child.txt
        let folder = Uuid::now_v7();
        vfs_nodes::insert()
            .id(folder)
            .name("folder")
            .kind(ObjectKind::Directory)
            .owner(owner)
            .created_at(100)
            .execute(&db)
            .await
            .unwrap();
        let child = Uuid::now_v7();
        vfs_nodes::insert()
            .id(child)
            .name("child.txt")
            .kind(ObjectKind::File)
            .parent_node(Some(folder))
            .owner(owner)
            .created_at(100)
            .execute(&db)
            .await
            .unwrap();
        add_object(&db, child, 150).await;

        let config = format!(r#"{{"node":"{folder}"}}"#);
        let trigger = VfsChangeTrigger::parse(Some(&config), owner).unwrap();
        // A change to a file inside the watched folder fires.
        assert!(trigger.due(&db, 200, Some(100)).await);
        assert!(!trigger.due(&db, 200, Some(150)).await);
    }

    #[test]
    fn rejects_empty_config() {
        let owner = Uuid::now_v7();
        assert!(VfsChangeTrigger::parse(None, owner).is_err());
        assert!(VfsChangeTrigger::parse(Some("{}"), owner).is_err());
    }
}
