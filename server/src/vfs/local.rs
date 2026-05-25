use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use minisql::{ConnectionPool, Value};
use uuid::Uuid;

use super::{DirEntry, EntryKind};

pub struct LocalFileProvider {
    pub db: ConnectionPool,
    pub base: PathBuf,
    pub keep_versions: usize,
}

impl LocalFileProvider {
    pub fn new(db: ConnectionPool, base: PathBuf, keep_versions: usize) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&base)
            .with_context(|| format!("create VFS base dir {:?}", base))?;
        Ok(Self {
            db,
            base,
            keep_versions,
        })
    }

    /// Returns workspace id for the given thread, creating one if needed.
    pub async fn get_or_create_workspace(
        &self,
        thread_id: Uuid,
        project_id: Option<Uuid>,
        owner: Uuid,
    ) -> anyhow::Result<Uuid> {
        // prefer project workspace over thread workspace
        if let Some(pid) = project_id {
            if let Some(id) = self.find_workspace_for_project(pid).await? {
                return Ok(id);
            }
            return self.create_workspace(None, Some(pid), owner).await;
        }

        if let Some(id) = self.find_workspace_for_thread(thread_id).await? {
            return Ok(id);
        }
        self.create_workspace(Some(thread_id), None, owner).await
    }

    /// Lists entries at `path` (relative to workspace root, empty string = root).
    /// Path `/` or empty → workspace root. Returns `~workspace` entry when called
    /// with the sentinel path `"/"` from the tool layer.
    pub async fn list(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        let segments = split_path(path);
        let parent_node = self.resolve_dir(workspace_id, &segments).await?;
        self.list_children(workspace_id, parent_node).await
    }

    /// Reads UTF-8 content of a file at `path` relative to workspace root.
    pub async fn read(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<String> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path is a directory");
        }
        let (dir_segs, file_name) = segments.split_at(segments.len() - 1);
        let parent = self.resolve_dir(workspace_id, dir_segs).await?;

        let node_id = self
            .find_child(workspace_id, parent, file_name[0])
            .await?
            .ok_or_else(|| anyhow::anyhow!("file not found: {path}"))?;

        let location = self
            .latest_object_location(node_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("file has no content: {path}"))?;

        let content = tokio::fs::read_to_string(self.base.join(&location))
            .await
            .with_context(|| format!("read file {location}"))?;
        Ok(content)
    }

    /// Writes UTF-8 content to a file at `path` relative to workspace root.
    /// Creates intermediate directories and new file nodes as needed.
    pub async fn write(
        &self,
        workspace_id: Uuid,
        path: &str,
        content: &str,
        owner: Uuid,
    ) -> anyhow::Result<()> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path must include a file name");
        }
        let (dir_segs, file_part) = segments.split_at(segments.len() - 1);
        let file_name = file_part[0];

        let parent = self.ensure_dir_path(workspace_id, dir_segs, owner).await?;

        let node_id = match self.find_child(workspace_id, parent, file_name).await? {
            Some(id) => id,
            None => {
                self.create_node(workspace_id, parent, file_name, "file", None, owner)
                    .await?
            }
        };

        let object_id = Uuid::now_v7();
        let location = object_id.as_simple().to_string();
        let full_path = self.base.join(&location);

        tokio::fs::write(&full_path, content)
            .await
            .with_context(|| format!("write object {location}"))?;

        let version = self.next_version(node_id).await?;
        let now = now_ms();
        let size = content.len() as i64;

        self.db
            .query_with_params(
                "INSERT INTO vfs_objects (id, version, location, created_at, node, size) VALUES (?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(object_id),
                    Value::Integer(version),
                    Value::Text(location),
                    Value::Integer(now),
                    Value::Uuid(node_id),
                    Value::Integer(size),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        self.prune_versions(node_id).await?;
        Ok(())
    }

    async fn find_workspace_for_project(&self, project_id: Uuid) -> anyhow::Result<Option<Uuid>> {
        let rows = self
            .db
            .query_with_params(
                "SELECT id FROM vfs_workspaces WHERE parent_project = ? LIMIT 1",
                vec![Value::Uuid(project_id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(rows.rows().first().and_then(|r| uuid_from_row(r, "id")))
    }

    async fn find_workspace_for_thread(&self, thread_id: Uuid) -> anyhow::Result<Option<Uuid>> {
        let rows = self
            .db
            .query_with_params(
                "SELECT id FROM vfs_workspaces WHERE parent_thread = ? LIMIT 1",
                vec![Value::Uuid(thread_id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(rows.rows().first().and_then(|r| uuid_from_row(r, "id")))
    }

    async fn create_workspace(
        &self,
        thread_id: Option<Uuid>,
        project_id: Option<Uuid>,
        owner: Uuid,
    ) -> anyhow::Result<Uuid> {
        let workspace_id = Uuid::now_v7();
        self.db
            .query_with_params(
                "INSERT INTO vfs_workspaces (id, parent_thread, parent_project) VALUES (?, ?, ?)",
                vec![
                    Value::Uuid(workspace_id),
                    opt_uuid(thread_id),
                    opt_uuid(project_id),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        // create root directory node for the workspace
        let root_id = Uuid::now_v7();
        let now = now_ms();
        self.db
            .query_with_params(
                "INSERT INTO vfs_nodes (id, name, kind, parent_node, parent_workspace, owner, created_at, mime_type) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(root_id),
                    Value::Text("".to_string()),
                    Value::Text("dir".to_string()),
                    Value::Null,
                    Value::Uuid(workspace_id),
                    Value::Uuid(owner),
                    Value::Integer(now),
                    Value::Null,
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        Ok(workspace_id)
    }

    /// Resolves a sequence of path segments to a directory node id.
    /// Returns `None` for workspace root (no explicit root node id needed for queries).
    async fn resolve_dir(
        &self,
        workspace_id: Uuid,
        segments: &[&str],
    ) -> anyhow::Result<Option<Uuid>> {
        let mut current: Option<Uuid> = None; // None = workspace root
        for &seg in segments {
            let child = self
                .find_child(workspace_id, current, seg)
                .await?
                .ok_or_else(|| anyhow::anyhow!("directory not found: {seg}"))?;
            current = Some(child);
        }
        Ok(current)
    }

    async fn find_child(
        &self,
        workspace_id: Uuid,
        parent: Option<Uuid>,
        name: &str,
    ) -> anyhow::Result<Option<Uuid>> {
        let (sql, params) = if let Some(pid) = parent {
            (
                "SELECT id FROM vfs_nodes WHERE parent_workspace = ? AND parent_node = ? AND name = ? LIMIT 1",
                vec![
                    Value::Uuid(workspace_id),
                    Value::Uuid(pid),
                    Value::Text(name.to_string()),
                ],
            )
        } else {
            (
                "SELECT id FROM vfs_nodes WHERE parent_workspace = ? AND parent_node IS NULL AND name = ? LIMIT 1",
                vec![Value::Uuid(workspace_id), Value::Text(name.to_string())],
            )
        };

        let rows = self
            .db
            .query_with_params(sql, params)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(rows.rows().first().and_then(|r| uuid_from_row(r, "id")))
    }

    async fn list_children(
        &self,
        workspace_id: Uuid,
        parent: Option<Uuid>,
    ) -> anyhow::Result<Vec<DirEntry>> {
        let (sql, params) = if let Some(pid) = parent {
            (
                "SELECT n.id, n.name, n.kind, n.mime_type, MAX(o.size) as size \
                 FROM vfs_nodes n LEFT JOIN vfs_objects o ON o.node = n.id \
                 WHERE n.parent_workspace = ? AND n.parent_node = ? \
                 GROUP BY n.id ORDER BY n.name ASC",
                vec![Value::Uuid(workspace_id), Value::Uuid(pid)],
            )
        } else {
            (
                "SELECT n.id, n.name, n.kind, n.mime_type, MAX(o.size) as size \
                 FROM vfs_nodes n LEFT JOIN vfs_objects o ON o.node = n.id \
                 WHERE n.parent_workspace = ? AND n.parent_node IS NULL AND n.name != '' \
                 GROUP BY n.id ORDER BY n.name ASC",
                vec![Value::Uuid(workspace_id)],
            )
        };

        let rows = self
            .db
            .query_with_params(sql, params)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let entries = rows
            .rows()
            .iter()
            .map(|row| {
                let kind = match row.get_text("kind") {
                    Some("dir") => EntryKind::Directory,
                    _ => EntryKind::File,
                };
                DirEntry {
                    name: row.get_text("name").unwrap_or_default().to_string(),
                    kind,
                    size: row.get_int("size"),
                    mime_type: row.get_text("mime_type").map(|s| s.to_string()),
                }
            })
            .collect();
        Ok(entries)
    }

    async fn ensure_dir_path(
        &self,
        workspace_id: Uuid,
        segments: &[&str],
        owner: Uuid,
    ) -> anyhow::Result<Option<Uuid>> {
        let mut current: Option<Uuid> = None;
        for &seg in segments {
            let child = match self.find_child(workspace_id, current, seg).await? {
                Some(id) => id,
                None => {
                    self.create_node(workspace_id, current, seg, "dir", None, owner)
                        .await?
                }
            };
            current = Some(child);
        }
        Ok(current)
    }

    async fn create_node(
        &self,
        workspace_id: Uuid,
        parent: Option<Uuid>,
        name: &str,
        kind: &str,
        mime_type: Option<&str>,
        owner: Uuid,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::now_v7();
        let now = now_ms();
        self.db
            .query_with_params(
                "INSERT INTO vfs_nodes (id, name, kind, parent_node, parent_workspace, owner, created_at, mime_type) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(id),
                    Value::Text(name.to_string()),
                    Value::Text(kind.to_string()),
                    parent.map(Value::Uuid).unwrap_or(Value::Null),
                    Value::Uuid(workspace_id),
                    Value::Uuid(owner),
                    Value::Integer(now),
                    mime_type.map(|s| Value::Text(s.to_string())).unwrap_or(Value::Null),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(id)
    }

    async fn latest_object_location(&self, node_id: Uuid) -> anyhow::Result<Option<String>> {
        let rows = self
            .db
            .query_with_params(
                "SELECT location FROM vfs_objects WHERE node = ? ORDER BY version DESC LIMIT 1",
                vec![Value::Uuid(node_id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(rows
            .rows()
            .first()
            .and_then(|r| r.get_text("location").map(|s| s.to_string())))
    }

    async fn next_version(&self, node_id: Uuid) -> anyhow::Result<i64> {
        let rows = self
            .db
            .query_with_params(
                "SELECT MAX(version) as v FROM vfs_objects WHERE node = ?",
                vec![Value::Uuid(node_id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let max = rows
            .rows()
            .first()
            .and_then(|r| r.get_int("v"))
            .unwrap_or(-1);
        Ok(max + 1)
    }

    async fn prune_versions(&self, node_id: Uuid) -> anyhow::Result<()> {
        let keep = self.keep_versions as i64;
        let rows = self
            .db
            .query_with_params(
                "SELECT id, location FROM vfs_objects WHERE node = ? ORDER BY version DESC LIMIT -1 OFFSET ?",
                vec![Value::Uuid(node_id), Value::Integer(keep)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        for row in rows.rows() {
            let Some(id) = uuid_from_row(row, "id") else {
                continue;
            };
            let location = row.get_text("location").map(|s| s.to_string());
            self.db
                .query_with_params(
                    "DELETE FROM vfs_objects WHERE id = ?",
                    vec![Value::Uuid(id)],
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if let Some(loc) = location {
                let _ = tokio::fs::remove_file(self.base.join(&loc)).await;
            }
        }
        Ok(())
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn opt_uuid(id: Option<Uuid>) -> Value {
    id.map(Value::Uuid).unwrap_or(Value::Null)
}

fn uuid_from_row(row: &minisql::Row, col: &str) -> Option<Uuid> {
    match row.get(col) {
        Some(Value::Uuid(id)) => Some(*id),
        Some(Value::Blob(bytes)) if bytes.len() == 16 => Uuid::from_slice(bytes).ok(),
        Some(Value::Text(s)) => Uuid::parse_str(s).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use minisql::ConnectionPool;

    async fn setup() -> (LocalFileProvider, Uuid) {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();

        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();

        let base = tempfile::tempdir().unwrap().into_path();
        let provider = LocalFileProvider::new(db, base, 3).unwrap();
        (provider, owner)
    }

    #[tokio::test]
    async fn workspace_created_once_per_thread() {
        let (p, owner) = setup().await;
        let thread_id = Uuid::now_v7();
        let w1 = p
            .get_or_create_workspace(thread_id, None, owner)
            .await
            .unwrap();
        let w2 = p
            .get_or_create_workspace(thread_id, None, owner)
            .await
            .unwrap();
        assert_eq!(w1, w2);
    }

    #[tokio::test]
    async fn project_workspace_shared_across_threads() {
        let (p, owner) = setup().await;
        let project_id = Uuid::now_v7();
        let t1 = Uuid::now_v7();
        let t2 = Uuid::now_v7();
        let w1 = p
            .get_or_create_workspace(t1, Some(project_id), owner)
            .await
            .unwrap();
        let w2 = p
            .get_or_create_workspace(t2, Some(project_id), owner)
            .await
            .unwrap();
        assert_eq!(w1, w2);
    }

    #[tokio::test]
    async fn write_and_read_file() {
        let (p, owner) = setup().await;
        let ws = p
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        p.write(ws, "hello.txt", "hello world", owner)
            .await
            .unwrap();
        let content = p.read(ws, "hello.txt").await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn write_creates_intermediate_dirs() {
        let (p, owner) = setup().await;
        let ws = p
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        p.write(ws, "a/b/c.txt", "deep", owner).await.unwrap();
        let content = p.read(ws, "a/b/c.txt").await.unwrap();
        assert_eq!(content, "deep");
    }

    #[tokio::test]
    async fn list_shows_entries() {
        let (p, owner) = setup().await;
        let ws = p
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        p.write(ws, "a.txt", "a", owner).await.unwrap();
        p.write(ws, "subdir/b.txt", "b", owner).await.unwrap();
        let entries = p.list(ws, "").await.unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"subdir"));
    }

    #[tokio::test]
    async fn versioning_prunes_old_objects() {
        let (p, owner) = setup().await;
        let ws = p
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        for i in 0..5 {
            p.write(ws, "f.txt", &format!("v{i}"), owner).await.unwrap();
        }
        let rows = p
            .db
            .query_with_params(
                "SELECT COUNT(*) as cnt FROM vfs_objects o JOIN vfs_nodes n ON n.id = o.node WHERE n.parent_workspace = ?",
                vec![Value::Uuid(ws)],
            )
            .await
            .unwrap();
        let count = rows
            .rows()
            .first()
            .and_then(|r| r.get_int("cnt"))
            .unwrap_or(0);
        assert_eq!(count, 3);
        let content = p.read(ws, "f.txt").await.unwrap();
        assert_eq!(content, "v4");
    }
}
