mod local;

pub use local::LocalFileProvider;

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use minisql::{ConnectionPool, Value};
use uuid::Uuid;

pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<i64>,
    pub mime_type: Option<String>,
}

pub enum EntryKind {
    Directory,
    File,
}

/// Byte-level storage backend. Implement this for each storage target.
pub trait FileProvider {
    /// Store bytes and return an opaque location key for later retrieval.
    async fn store(&self, content: &[u8]) -> anyhow::Result<String>;
    /// Load bytes by location key.
    async fn load(&self, location: &str) -> anyhow::Result<Vec<u8>>;
    /// Delete stored bytes by location key.
    async fn delete(&self, location: &str) -> anyhow::Result<()>;
}

/// Enum dispatch over concrete storage backends.
pub enum AnyFileProvider {
    Local(LocalFileProvider),
}

impl FileProvider for AnyFileProvider {
    async fn store(&self, content: &[u8]) -> anyhow::Result<String> {
        match self {
            AnyFileProvider::Local(p) => p.store(content).await,
        }
    }

    async fn load(&self, location: &str) -> anyhow::Result<Vec<u8>> {
        match self {
            AnyFileProvider::Local(p) => p.load(location).await,
        }
    }

    async fn delete(&self, location: &str) -> anyhow::Result<()> {
        match self {
            AnyFileProvider::Local(p) => p.delete(location).await,
        }
    }
}

/// VFS service: DB metadata + a pluggable storage backend.
pub struct Vfs {
    db: ConnectionPool,
    storage: AnyFileProvider,
    keep_versions: usize,
}

impl Vfs {
    pub fn new(db: ConnectionPool, storage: AnyFileProvider, keep_versions: usize) -> Self {
        Self {
            db,
            storage,
            keep_versions,
        }
    }

    /// Returns workspace id for the given thread, creating one if needed.
    pub async fn get_or_create_workspace(
        &self,
        thread_id: Uuid,
        project_id: Option<Uuid>,
        owner: Uuid,
    ) -> anyhow::Result<Uuid> {
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

    /// Lists entries at `path` relative to workspace root (empty = root).
    pub async fn list(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        let segments = split_path(path);
        let parent = self.resolve_dir(workspace_id, &segments).await?;
        self.list_children(workspace_id, parent).await
    }

    /// Reads UTF-8 content of a file at `path` relative to workspace root.
    pub async fn read(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<String> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path is a directory");
        }
        let (dir_segs, file_part) = segments.split_at(segments.len() - 1);
        let parent = self.resolve_dir(workspace_id, dir_segs).await?;

        let node_id = self
            .find_child(workspace_id, parent, file_part[0])
            .await?
            .ok_or_else(|| anyhow::anyhow!("file not found: {path}"))?;

        let location = self
            .latest_object_location(node_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("file has no content: {path}"))?;

        let bytes = self
            .storage
            .load(&location)
            .await
            .with_context(|| format!("load {location}"))?;
        String::from_utf8(bytes).context("file is not valid UTF-8")
    }

    /// Writes UTF-8 content to `path` relative to workspace root.
    /// Creates intermediate directories and file nodes as needed.
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

        let location = self.storage.store(content.as_bytes()).await?;
        let version = self.next_version(node_id).await?;
        let size = content.len() as i64;

        self.db
            .query_with_params(
                "INSERT INTO vfs_objects (id, version, location, created_at, node, size) VALUES (?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(Uuid::now_v7()),
                    Value::Integer(version),
                    Value::Text(location),
                    Value::Integer(now_ms()),
                    Value::Uuid(node_id),
                    Value::Integer(size),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        self.prune_versions(node_id).await
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

        self.db
            .query_with_params(
                "INSERT INTO vfs_nodes (id, name, kind, parent_node, parent_workspace, owner, created_at, mime_type) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(Uuid::now_v7()),
                    Value::Text("".to_string()),
                    Value::Text("dir".to_string()),
                    Value::Null,
                    Value::Uuid(workspace_id),
                    Value::Uuid(owner),
                    Value::Integer(now_ms()),
                    Value::Null,
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        Ok(workspace_id)
    }

    async fn resolve_dir(
        &self,
        workspace_id: Uuid,
        segments: &[&str],
    ) -> anyhow::Result<Option<Uuid>> {
        let mut current = None;
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
                "SELECT n.name, n.kind, n.mime_type, MAX(o.size) as size \
                 FROM vfs_nodes n LEFT JOIN vfs_objects o ON o.node = n.id \
                 WHERE n.parent_workspace = ? AND n.parent_node = ? \
                 GROUP BY n.id ORDER BY n.name ASC",
                vec![Value::Uuid(workspace_id), Value::Uuid(pid)],
            )
        } else {
            (
                "SELECT n.name, n.kind, n.mime_type, MAX(o.size) as size \
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
        Ok(rows
            .rows()
            .iter()
            .map(|row| DirEntry {
                name: row.get_text("name").unwrap_or_default().to_string(),
                kind: match row.get_text("kind") {
                    Some("dir") => EntryKind::Directory,
                    _ => EntryKind::File,
                },
                size: row.get_int("size"),
                mime_type: row.get_text("mime_type").map(|s| s.to_string()),
            })
            .collect())
    }

    async fn ensure_dir_path(
        &self,
        workspace_id: Uuid,
        segments: &[&str],
        owner: Uuid,
    ) -> anyhow::Result<Option<Uuid>> {
        let mut current = None;
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
                    Value::Integer(now_ms()),
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
        Ok(rows
            .rows()
            .first()
            .and_then(|r| r.get_int("v"))
            .unwrap_or(-1)
            + 1)
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
                let _ = self.storage.delete(&loc).await;
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

    async fn setup_vfs() -> (Vfs, Uuid) {
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
        let storage = AnyFileProvider::Local(LocalFileProvider::new(base).unwrap());
        let vfs = Vfs::new(db, storage, 3);
        (vfs, owner)
    }

    #[tokio::test]
    async fn workspace_created_once_per_thread() {
        let (vfs, owner) = setup_vfs().await;
        let thread_id = Uuid::now_v7();
        let w1 = vfs
            .get_or_create_workspace(thread_id, None, owner)
            .await
            .unwrap();
        let w2 = vfs
            .get_or_create_workspace(thread_id, None, owner)
            .await
            .unwrap();
        assert_eq!(w1, w2);
    }

    #[tokio::test]
    async fn project_workspace_shared_across_threads() {
        let (vfs, owner) = setup_vfs().await;
        let project_id = Uuid::now_v7();
        let w1 = vfs
            .get_or_create_workspace(Uuid::now_v7(), Some(project_id), owner)
            .await
            .unwrap();
        let w2 = vfs
            .get_or_create_workspace(Uuid::now_v7(), Some(project_id), owner)
            .await
            .unwrap();
        assert_eq!(w1, w2);
    }

    #[tokio::test]
    async fn write_and_read_file() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.write(ws, "hello.txt", "hello world", owner)
            .await
            .unwrap();
        let content = vfs.read(ws, "hello.txt").await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn write_creates_intermediate_dirs() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.write(ws, "a/b/c.txt", "deep", owner).await.unwrap();
        assert_eq!(vfs.read(ws, "a/b/c.txt").await.unwrap(), "deep");
    }

    #[tokio::test]
    async fn list_shows_entries() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.write(ws, "a.txt", "a", owner).await.unwrap();
        vfs.write(ws, "subdir/b.txt", "b", owner).await.unwrap();
        let names: Vec<_> = vfs
            .list(ws, "")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"a.txt".to_string()));
        assert!(names.contains(&"subdir".to_string()));
    }

    #[tokio::test]
    async fn versioning_prunes_old_objects() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        for i in 0..5 {
            vfs.write(ws, "f.txt", &format!("v{i}"), owner)
                .await
                .unwrap();
        }
        let rows = vfs.db
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
        assert_eq!(vfs.read(ws, "f.txt").await.unwrap(), "v4");
    }
}
