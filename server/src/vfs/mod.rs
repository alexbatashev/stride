mod local;
mod mounted;

pub use local::LocalFileProvider;
pub use mounted::{MountedVfs, WORKSPACE_MOUNT, WritableArea};

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use minisql::{ConnectionPool, Value};
use uuid::Uuid;

use crate::db::{vfs_nodes, vfs_objects};

pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<i64>,
    pub updated_at: i64,
    pub mime_type: Option<String>,
}

pub enum EntryKind {
    Directory,
    File,
}

/// A file uploaded before any thread exists. Referenced by id when the owner
/// later creates or messages a thread.
pub struct StagedUpload {
    pub id: Uuid,
    pub name: String,
    pub size: i64,
}

/// The contents of a staged upload, taken out of the staging area so the caller
/// can move it into a thread's workspace.
pub struct StagedFile {
    pub name: String,
    pub mime_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// Addresses a node tree: either a thread/project workspace or a user's global
/// space (nodes with `parent_workspace` NULL, owned by the user).
#[derive(Clone, Copy)]
pub enum Scope {
    Workspace(Uuid),
    Global(Uuid),
}

impl Scope {
    /// Builds the `WHERE` fragment and params selecting nodes in this scope.
    /// `prefix` is the column qualifier (e.g. "n." or "").
    fn clause(&self, prefix: &str) -> (String, Vec<Value>) {
        match self {
            Scope::Workspace(id) => (
                format!("{prefix}parent_workspace = ?"),
                vec![Value::Uuid(*id)],
            ),
            Scope::Global(owner) => (
                format!("{prefix}parent_workspace IS NULL AND {prefix}owner = ?"),
                vec![Value::Uuid(*owner)],
            ),
        }
    }

    /// Value stored in `vfs_nodes.parent_workspace` for nodes in this scope.
    fn parent_workspace(&self) -> Value {
        match self {
            Scope::Workspace(id) => Value::Uuid(*id),
            Scope::Global(_) => Value::Null,
        }
    }
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

    /// Stores raw bytes in the backend and returns an opaque location key. Used
    /// for content (such as published images) that lives outside the node tree.
    pub async fn store_blob(&self, content: &[u8]) -> anyhow::Result<String> {
        self.storage.store(content).await
    }

    /// Loads raw bytes previously stored via [`Vfs::store_blob`].
    pub async fn load_blob(&self, location: &str) -> anyhow::Result<Vec<u8>> {
        self.storage.load(location).await
    }

    /// Stores an uploaded file in the staging area, detached from any thread,
    /// and returns a handle the caller references when a thread is created.
    pub async fn stage_upload(
        &self,
        owner: Uuid,
        name: &str,
        mime_type: Option<&str>,
        content: &[u8],
    ) -> anyhow::Result<StagedUpload> {
        let location = self.storage.store(content).await?;
        let id = Uuid::now_v7();
        let size = content.len() as i64;
        if let Err(error) = self
            .db
            .query_with_params(
                "INSERT INTO staged_uploads (id, owner, name, mime_type, location, size, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(id),
                    Value::Uuid(owner),
                    Value::Text(name.to_string()),
                    mime_type
                        .map(|m| Value::Text(m.to_string()))
                        .unwrap_or(Value::Null),
                    Value::Text(location.clone()),
                    Value::Integer(size),
                    Value::Integer(now_ms()),
                ],
            )
            .await
        {
            let _ = self.storage.delete(&location).await;
            bail!(error.to_string());
        }

        Ok(StagedUpload {
            id,
            name: name.to_string(),
            size,
        })
    }

    /// Removes a staged upload owned by `owner` and returns its contents so the
    /// caller can write it into a thread's workspace. The staging row and its
    /// blob are deleted; an unknown or non-owned id is an error.
    pub async fn take_staged_upload(&self, owner: Uuid, id: Uuid) -> anyhow::Result<StagedFile> {
        let rows = self
            .db
            .query_with_params(
                "SELECT name, mime_type, location FROM staged_uploads WHERE id = ? AND owner = ? LIMIT 1",
                vec![Value::Uuid(id), Value::Uuid(owner)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let row = rows
            .rows()
            .first()
            .ok_or_else(|| anyhow::anyhow!("staged upload not found: {id}"))?;
        let name = row.get_text("name").unwrap_or_default().to_string();
        let mime_type = row.get_text("mime_type").map(|s| s.to_string());
        let location = row
            .get_text("location")
            .ok_or_else(|| anyhow::anyhow!("staged upload missing location: {id}"))?
            .to_string();

        let bytes = self.storage.load(&location).await?;
        self.db
            .query_with_params(
                "DELETE FROM staged_uploads WHERE id = ?",
                vec![Value::Uuid(id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let _ = self.storage.delete(&location).await;

        Ok(StagedFile {
            name,
            mime_type,
            bytes,
        })
    }

    /// Deletes staged uploads created before `cutoff_ms`, along with their stored
    /// blobs, and returns how many were removed.
    pub async fn cleanup_staged_uploads(&self, cutoff_ms: i64) -> anyhow::Result<usize> {
        let rows = self
            .db
            .query_with_params(
                "SELECT id, location FROM staged_uploads WHERE created_at < ?",
                vec![Value::Integer(cutoff_ms)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let mut removed = 0;
        for row in rows.rows() {
            let Some(id) = uuid_from_row(row, "id") else {
                continue;
            };
            let location = row.get_text("location").map(|s| s.to_string());
            self.db
                .query_with_params(
                    "DELETE FROM staged_uploads WHERE id = ?",
                    vec![Value::Uuid(id)],
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if let Some(loc) = location {
                let _ = self.storage.delete(&loc).await;
            }
            removed += 1;
        }
        Ok(removed)
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

    /// Ensures the project's folder exists in the owner's global files and
    /// returns its global path (e.g. `Projects/Acme`).
    pub async fn ensure_project_dir(&self, owner: Uuid, title: &str) -> anyhow::Result<String> {
        let prefix = mounted::project_dir_prefix(title);
        self.create_dir_global(owner, &prefix).await?;
        Ok(prefix)
    }

    /// Renames a project's global folder when the project title changes. Best
    /// effort: a missing source folder is created under the new name.
    pub async fn rename_project_dir(
        &self,
        owner: Uuid,
        old_title: &str,
        new_title: &str,
    ) -> anyhow::Result<String> {
        let old_name = mounted::project_dir_name(old_title);
        let new_name = mounted::project_dir_name(new_title);
        let new_prefix = mounted::project_dir_prefix(new_title);
        if old_name == new_name {
            self.create_dir_global(owner, &new_prefix).await?;
            return Ok(new_prefix);
        }
        let old_prefix = mounted::project_dir_prefix(old_title);
        match self.rename_global(owner, &old_prefix, &new_name).await {
            Ok(()) => Ok(new_prefix),
            Err(_) => {
                // Source missing or target exists; fall back to ensuring the
                // destination so callers always get a usable folder.
                self.create_dir_global(owner, &new_prefix).await?;
                Ok(new_prefix)
            }
        }
    }

    /// Lists a writable area at `rel` (relative to its root). Routes to the
    /// standalone workspace or the project's global subtree.
    pub async fn area_list(
        &self,
        area: &WritableArea,
        owner: Uuid,
        rel: &str,
    ) -> anyhow::Result<Vec<DirEntry>> {
        match area {
            WritableArea::Workspace(id) => self.list(*id, rel).await,
            WritableArea::ProjectDir(prefix) => {
                self.list_global(owner, &join_under(prefix, rel)).await
            }
        }
    }

    /// Reads raw bytes and mime type from a writable area at `rel`.
    pub async fn area_read_bytes(
        &self,
        area: &WritableArea,
        owner: Uuid,
        rel: &str,
    ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        match area {
            WritableArea::Workspace(id) => self.read_bytes(*id, rel).await,
            WritableArea::ProjectDir(prefix) => {
                self.read_bytes_global(owner, &join_under(prefix, rel))
                    .await
            }
        }
    }

    /// Writes raw bytes to a writable area at `rel`.
    pub async fn area_write_bytes(
        &self,
        area: &WritableArea,
        owner: Uuid,
        rel: &str,
        content: &[u8],
        mime_type: Option<&str>,
    ) -> anyhow::Result<()> {
        match area {
            WritableArea::Workspace(id) => {
                self.write_bytes(*id, rel, content, mime_type, owner).await
            }
            WritableArea::ProjectDir(prefix) => {
                self.write_bytes_global(owner, &join_under(prefix, rel), content, mime_type)
                    .await
            }
        }
    }

    /// Creates a directory (and parents) in a writable area at `rel`.
    pub async fn area_create_dir(
        &self,
        area: &WritableArea,
        owner: Uuid,
        rel: &str,
    ) -> anyhow::Result<()> {
        match area {
            WritableArea::Workspace(id) => self.create_dir(*id, rel, owner).await,
            WritableArea::ProjectDir(prefix) => {
                self.create_dir_global(owner, &join_under(prefix, rel))
                    .await
            }
        }
    }

    /// Deletes a file or directory tree in a writable area at `rel`.
    pub async fn area_delete(
        &self,
        area: &WritableArea,
        owner: Uuid,
        rel: &str,
    ) -> anyhow::Result<()> {
        match area {
            WritableArea::Workspace(id) => self.delete(*id, rel).await,
            WritableArea::ProjectDir(prefix) => {
                self.delete_global(owner, &join_under(prefix, rel)).await
            }
        }
    }

    /// Lists entries at `path` relative to workspace root (empty = root).
    pub async fn list(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        self.list_scoped(Scope::Workspace(workspace_id), path).await
    }

    /// Lists entries in the user's global space at `path` (empty = root).
    pub async fn list_global(&self, owner: Uuid, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        self.list_scoped(Scope::Global(owner), path).await
    }

    /// Resolves a file or directory `path` in the user's global space to its
    /// node id. Used to pin a vfs_change watch target.
    pub async fn resolve_global_node(&self, owner: Uuid, path: &str) -> anyhow::Result<Uuid> {
        let scope = Scope::Global(owner);
        let segments = split_path(path);
        let Some(last) = segments.last().copied() else {
            bail!("path is empty");
        };
        let dirs = &segments[..segments.len() - 1];
        let parent = self.resolve_dir(scope, dirs).await?;
        self.find_child(scope, parent, last)
            .await?
            .ok_or_else(|| anyhow::anyhow!("path not found: {path}"))
    }

    async fn list_scoped(&self, scope: Scope, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        let segments = split_path(path);
        let parent = self.resolve_dir(scope, &segments).await?;
        self.list_children(scope, parent).await
    }

    /// Creates a directory and any missing parent directories.
    pub async fn create_dir(
        &self,
        workspace_id: Uuid,
        path: &str,
        owner: Uuid,
    ) -> anyhow::Result<()> {
        self.create_dir_scoped(Scope::Workspace(workspace_id), path, owner)
            .await
    }

    /// Creates a directory in the user's global space.
    pub async fn create_dir_global(&self, owner: Uuid, path: &str) -> anyhow::Result<()> {
        self.create_dir_scoped(Scope::Global(owner), path, owner)
            .await
    }

    async fn create_dir_scoped(&self, scope: Scope, path: &str, owner: Uuid) -> anyhow::Result<()> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path must include a directory name");
        }

        self.ensure_dir_path(scope, &segments, owner).await?;
        Ok(())
    }

    /// Renames a node (file or directory) at `path` to `new_name`.
    pub async fn rename(
        &self,
        workspace_id: Uuid,
        path: &str,
        new_name: &str,
    ) -> anyhow::Result<()> {
        self.rename_scoped(Scope::Workspace(workspace_id), path, new_name)
            .await
    }

    /// Renames a node in the user's global space.
    pub async fn rename_global(
        &self,
        owner: Uuid,
        path: &str,
        new_name: &str,
    ) -> anyhow::Result<()> {
        self.rename_scoped(Scope::Global(owner), path, new_name)
            .await
    }

    async fn rename_scoped(&self, scope: Scope, path: &str, new_name: &str) -> anyhow::Result<()> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path must include a file or directory name");
        }
        if new_name.is_empty() || new_name.contains('/') {
            bail!("invalid name: {new_name}");
        }

        let (dir_segs, name_part) = segments.split_at(segments.len() - 1);
        let parent = self.resolve_dir(scope, dir_segs).await?;
        let node_id = self
            .find_child(scope, parent, name_part[0])
            .await?
            .ok_or_else(|| anyhow::anyhow!("path not found: {path}"))?;
        if self.find_child(scope, parent, new_name).await?.is_some() {
            bail!("name already exists: {new_name}");
        }

        vfs_nodes::update()
            .name(new_name)
            .where_(vfs_nodes::id.eq(node_id))
            .execute(&self.db)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(())
    }

    /// Deletes a file or directory tree at `path` relative to workspace root.
    pub async fn delete(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<()> {
        self.delete_scoped(Scope::Workspace(workspace_id), path)
            .await
    }

    /// Deletes a file or directory tree in the user's global space.
    pub async fn delete_global(&self, owner: Uuid, path: &str) -> anyhow::Result<()> {
        self.delete_scoped(Scope::Global(owner), path).await
    }

    async fn delete_scoped(&self, scope: Scope, path: &str) -> anyhow::Result<()> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path must include a file or directory name");
        }

        let (dir_segs, name_part) = segments.split_at(segments.len() - 1);
        let parent = self.resolve_dir(scope, dir_segs).await?;
        let node_id = self
            .find_child(scope, parent, name_part[0])
            .await?
            .ok_or_else(|| anyhow::anyhow!("path not found: {path}"))?;

        let mut node_ids = Vec::new();
        self.collect_descendants(scope, node_id, &mut node_ids)
            .await?;

        for id in &node_ids {
            let rows = self
                .db
                .query_with_params(
                    "SELECT id, location FROM vfs_objects WHERE node = ?",
                    vec![Value::Uuid(*id)],
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            for row in rows.rows() {
                let Some(object_id) = uuid_from_row(row, "id") else {
                    continue;
                };
                let location = row.get_text("location").map(|s| s.to_string());
                vfs_objects::delete()
                    .where_(vfs_objects::id.eq(object_id))
                    .execute(&self.db)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                if let Some(loc) = location {
                    let _ = self.storage.delete(&loc).await;
                }
            }
        }

        for id in node_ids.into_iter().rev() {
            vfs_nodes::delete()
                .where_(vfs_nodes::id.eq(id))
                .execute(&self.db)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }

        Ok(())
    }

    /// Reads UTF-8 content of a file at `path` relative to workspace root.
    pub async fn read(&self, workspace_id: Uuid, path: &str) -> anyhow::Result<String> {
        let (bytes, _) = self.read_bytes(workspace_id, path).await?;
        String::from_utf8(bytes).context("file is not valid UTF-8")
    }

    /// Reads UTF-8 content of a file in the user's global space.
    pub async fn read_global(&self, owner: Uuid, path: &str) -> anyhow::Result<String> {
        let (bytes, _) = self.read_bytes_scoped(Scope::Global(owner), path).await?;
        String::from_utf8(bytes).context("file is not valid UTF-8")
    }

    /// Reads raw bytes and mime type of a file at `path` relative to workspace root.
    pub async fn read_bytes(
        &self,
        workspace_id: Uuid,
        path: &str,
    ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        self.read_bytes_scoped(Scope::Workspace(workspace_id), path)
            .await
    }

    /// Reads raw bytes and mime type of a file in the user's global space.
    pub async fn read_bytes_global(
        &self,
        owner: Uuid,
        path: &str,
    ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        self.read_bytes_scoped(Scope::Global(owner), path).await
    }

    async fn read_bytes_scoped(
        &self,
        scope: Scope,
        path: &str,
    ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path is a directory");
        }
        let (dir_segs, file_part) = segments.split_at(segments.len() - 1);
        let parent = self.resolve_dir(scope, dir_segs).await?;

        let node_id = self
            .find_child(scope, parent, file_part[0])
            .await?
            .ok_or_else(|| anyhow::anyhow!("file not found: {path}"))?;
        if self.node_kind(node_id).await? != "file" {
            bail!("path is a directory");
        }

        let rows = self
            .db
            .query_with_params(
                "SELECT n.mime_type, o.location FROM vfs_nodes n \
                 JOIN vfs_objects o ON o.node = n.id \
                 WHERE n.id = ? ORDER BY o.version DESC LIMIT 1",
                vec![Value::Uuid(node_id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let row = rows
            .rows()
            .first()
            .ok_or_else(|| anyhow::anyhow!("file has no content: {path}"))?;
        let mime_type = row.get_text("mime_type").map(|s| s.to_string());
        let location = row
            .get_text("location")
            .ok_or_else(|| anyhow::anyhow!("missing location for: {path}"))?
            .to_string();

        let bytes = self
            .storage
            .load(&location)
            .await
            .with_context(|| format!("load {location}"))?;

        Ok((bytes, mime_type))
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
        self.write_bytes(workspace_id, path, content.as_bytes(), None, owner)
            .await
    }

    /// Writes UTF-8 content to `path` in the user's global space.
    pub async fn write_global(&self, owner: Uuid, path: &str, content: &str) -> anyhow::Result<()> {
        self.write_bytes_scoped(Scope::Global(owner), path, content.as_bytes(), None, owner)
            .await
    }

    /// Writes raw bytes to `path` relative to workspace root.
    /// Creates intermediate directories and file nodes as needed.
    pub async fn write_bytes(
        &self,
        workspace_id: Uuid,
        path: &str,
        content: &[u8],
        mime_type: Option<&str>,
        owner: Uuid,
    ) -> anyhow::Result<()> {
        self.write_bytes_scoped(
            Scope::Workspace(workspace_id),
            path,
            content,
            mime_type,
            owner,
        )
        .await
    }

    /// Writes raw bytes to `path` in the user's global space.
    pub async fn write_bytes_global(
        &self,
        owner: Uuid,
        path: &str,
        content: &[u8],
        mime_type: Option<&str>,
    ) -> anyhow::Result<()> {
        self.write_bytes_scoped(Scope::Global(owner), path, content, mime_type, owner)
            .await
    }

    async fn write_bytes_scoped(
        &self,
        scope: Scope,
        path: &str,
        content: &[u8],
        mime_type: Option<&str>,
        owner: Uuid,
    ) -> anyhow::Result<()> {
        let segments = split_path(path);
        if segments.is_empty() {
            bail!("path must include a file name");
        }
        let (dir_segs, file_part) = segments.split_at(segments.len() - 1);
        let file_name = file_part[0];

        let parent = self.ensure_dir_path(scope, dir_segs, owner).await?;
        let node_id = match self.find_child(scope, parent, file_name).await? {
            Some(id) => {
                if self.node_kind(id).await? != "file" {
                    bail!("path is a directory");
                }
                if let Some(mime) = mime_type {
                    vfs_nodes::update()
                        .mime_type(Some(mime))
                        .where_(vfs_nodes::id.eq(id))
                        .execute(&self.db)
                        .await
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                }
                id
            }
            None => {
                self.create_node(scope, parent, file_name, "file", mime_type, owner)
                    .await?
            }
        };

        let location = self.storage.store(content).await?;
        let version = self.next_version(node_id).await?;

        self.db
            .query_with_params(
                "INSERT INTO vfs_objects (id, version, location, created_at, node, size) VALUES (?, ?, ?, ?, ?, ?)",
                vec![
                    Value::Uuid(Uuid::now_v7()),
                    Value::Integer(version),
                    Value::Text(location),
                    Value::Integer(now_ms()),
                    Value::Uuid(node_id),
                    Value::Integer(content.len() as i64),
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

    async fn resolve_dir(&self, scope: Scope, segments: &[&str]) -> anyhow::Result<Option<Uuid>> {
        let mut current = None;
        for &seg in segments {
            let child = self
                .find_child(scope, current, seg)
                .await?
                .ok_or_else(|| anyhow::anyhow!("directory not found: {seg}"))?;
            if self.node_kind(child).await? != "dir" {
                bail!("path is not a directory: {seg}");
            }
            current = Some(child);
        }
        Ok(current)
    }

    async fn find_child(
        &self,
        scope: Scope,
        parent: Option<Uuid>,
        name: &str,
    ) -> anyhow::Result<Option<Uuid>> {
        let (scope_sql, mut params) = scope.clause("");
        let sql = if let Some(pid) = parent {
            params.push(Value::Uuid(pid));
            params.push(Value::Text(name.to_string()));
            format!(
                "SELECT id FROM vfs_nodes WHERE {scope_sql} AND parent_node = ? AND name = ? LIMIT 1"
            )
        } else {
            params.push(Value::Text(name.to_string()));
            format!(
                "SELECT id FROM vfs_nodes WHERE {scope_sql} AND parent_node IS NULL AND name = ? LIMIT 1"
            )
        };
        let rows = self
            .db
            .query_with_params(&sql, params)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(rows.rows().first().and_then(|r| uuid_from_row(r, "id")))
    }

    async fn list_children(
        &self,
        scope: Scope,
        parent: Option<Uuid>,
    ) -> anyhow::Result<Vec<DirEntry>> {
        let (scope_sql, mut params) = scope.clause("n.");
        let sql = if let Some(pid) = parent {
            params.push(Value::Uuid(pid));
            format!(
                "SELECT n.name, n.kind, n.mime_type, \
                    (SELECT o.size FROM vfs_objects o WHERE o.node = n.id ORDER BY o.version DESC LIMIT 1) as size, \
                    COALESCE((SELECT o.created_at FROM vfs_objects o WHERE o.node = n.id ORDER BY o.version DESC LIMIT 1), n.created_at) as updated_at \
                 FROM vfs_nodes n \
                 WHERE {scope_sql} AND n.parent_node = ? \
                 ORDER BY CASE n.kind WHEN 'dir' THEN 0 ELSE 1 END, n.name COLLATE NOCASE ASC"
            )
        } else {
            format!(
                "SELECT n.name, n.kind, n.mime_type, \
                    (SELECT o.size FROM vfs_objects o WHERE o.node = n.id ORDER BY o.version DESC LIMIT 1) as size, \
                    COALESCE((SELECT o.created_at FROM vfs_objects o WHERE o.node = n.id ORDER BY o.version DESC LIMIT 1), n.created_at) as updated_at \
                 FROM vfs_nodes n \
                 WHERE {scope_sql} AND n.parent_node IS NULL AND n.name != '' \
                 ORDER BY CASE n.kind WHEN 'dir' THEN 0 ELSE 1 END, n.name COLLATE NOCASE ASC"
            )
        };
        let rows = self
            .db
            .query_with_params(&sql, params)
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
                updated_at: row.get_int("updated_at").unwrap_or_default(),
                mime_type: row.get_text("mime_type").map(|s| s.to_string()),
            })
            .collect())
    }

    async fn ensure_dir_path(
        &self,
        scope: Scope,
        segments: &[&str],
        owner: Uuid,
    ) -> anyhow::Result<Option<Uuid>> {
        let mut current = None;
        for &seg in segments {
            let child = match self.find_child(scope, current, seg).await? {
                Some(id) => {
                    if self.node_kind(id).await? != "dir" {
                        bail!("path is not a directory: {seg}");
                    }
                    id
                }
                None => {
                    self.create_node(scope, current, seg, "dir", None, owner)
                        .await?
                }
            };
            current = Some(child);
        }
        Ok(current)
    }

    async fn create_node(
        &self,
        scope: Scope,
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
                    scope.parent_workspace(),
                    Value::Uuid(owner),
                    Value::Integer(now_ms()),
                    mime_type.map(|s| Value::Text(s.to_string())).unwrap_or(Value::Null),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(id)
    }

    async fn node_kind(&self, node_id: Uuid) -> anyhow::Result<String> {
        let rows = self
            .db
            .query_with_params(
                "SELECT kind FROM vfs_nodes WHERE id = ? LIMIT 1",
                vec![Value::Uuid(node_id)],
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        rows.rows()
            .first()
            .and_then(|r| r.get_text("kind"))
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("node not found"))
    }

    async fn collect_descendants(
        &self,
        scope: Scope,
        node_id: Uuid,
        node_ids: &mut Vec<Uuid>,
    ) -> anyhow::Result<()> {
        node_ids.push(node_id);
        let (scope_sql, mut params) = scope.clause("");
        params.push(Value::Uuid(node_id));
        let rows = self
            .db
            .query_with_params(
                &format!("SELECT id FROM vfs_nodes WHERE {scope_sql} AND parent_node = ?"),
                params,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        for row in rows.rows() {
            if let Some(child_id) = uuid_from_row(row, "id") {
                Box::pin(self.collect_descendants(scope, child_id, node_ids)).await?;
            }
        }

        Ok(())
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
            vfs_objects::delete()
                .where_(vfs_objects::id.eq(id))
                .execute(&self.db)
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

/// Joins a writable-root-relative path under a project's global prefix.
fn join_under(prefix: &str, rel: &str) -> String {
    if rel.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
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
    use std::sync::Arc;

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

        let base = tempfile::tempdir().unwrap().keep();
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
    async fn create_dir_adds_empty_directory() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.create_dir(ws, "a/b", owner).await.unwrap();
        let names: Vec<_> = vfs
            .list(ws, "a")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["b".to_string()]);
    }

    #[tokio::test]
    async fn delete_removes_directory_tree() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.write(ws, "a/b/c.txt", "deep", owner).await.unwrap();
        vfs.write(ws, "keep.txt", "keep", owner).await.unwrap();
        vfs.delete(ws, "a").await.unwrap();

        assert!(vfs.read(ws, "a/b/c.txt").await.is_err());
        assert_eq!(vfs.read(ws, "keep.txt").await.unwrap(), "keep");
    }

    #[tokio::test]
    async fn global_files_isolated_from_workspace() {
        let (vfs, owner) = setup_vfs().await;
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.write_global(owner, "doc.txt", "global").await.unwrap();
        vfs.write(ws, "doc.txt", "workspace", owner).await.unwrap();

        assert_eq!(vfs.read_global(owner, "doc.txt").await.unwrap(), "global");
        assert_eq!(vfs.read(ws, "doc.txt").await.unwrap(), "workspace");
        // Global file must not leak into the workspace listing.
        let ws_names: Vec<_> = vfs
            .list(ws, "")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(ws_names, vec!["doc.txt".to_string()]);
        let global_names: Vec<_> = vfs
            .list_global(owner, "")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(global_names, vec!["doc.txt".to_string()]);
    }

    #[tokio::test]
    async fn global_files_scoped_per_owner() {
        let (vfs, owner) = setup_vfs().await;
        let other = Uuid::now_v7();
        vfs.db
            .query_with_params(
                "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
                vec![
                    Value::Uuid(other),
                    Value::Text("bob".to_string()),
                    Value::Text("hash".to_string()),
                ],
            )
            .await
            .unwrap();
        vfs.write_global(owner, "a.txt", "alice").await.unwrap();
        assert!(vfs.list_global(other, "").await.unwrap().is_empty());
        assert!(vfs.read_global(other, "a.txt").await.is_err());
    }

    #[tokio::test]
    async fn rename_changes_name() {
        let (vfs, owner) = setup_vfs().await;
        vfs.write_global(owner, "dir/old.txt", "x").await.unwrap();
        vfs.rename_global(owner, "dir/old.txt", "new.txt")
            .await
            .unwrap();
        assert_eq!(vfs.read_global(owner, "dir/new.txt").await.unwrap(), "x");
        assert!(vfs.read_global(owner, "dir/old.txt").await.is_err());
    }

    #[tokio::test]
    async fn ensure_project_dir_appears_in_global_files() {
        let (vfs, owner) = setup_vfs().await;
        let prefix = vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        assert_eq!(prefix, "Projects/Acme");

        let roots: Vec<_> = vfs
            .list_global(owner, "")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(roots.contains(&"Projects".to_string()));
        let projects: Vec<_> = vfs
            .list_global(owner, "Projects")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(projects, vec!["Acme".to_string()]);
    }

    #[tokio::test]
    async fn rename_project_dir_moves_files() {
        let (vfs, owner) = setup_vfs().await;
        vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        vfs.write_global(owner, "Projects/Acme/report.txt", "hi")
            .await
            .unwrap();

        let prefix = vfs.rename_project_dir(owner, "Acme", "Beta").await.unwrap();
        assert_eq!(prefix, "Projects/Beta");
        assert_eq!(
            vfs.read_global(owner, "Projects/Beta/report.txt")
                .await
                .unwrap(),
            "hi"
        );
        assert!(
            vfs.read_global(owner, "Projects/Acme/report.txt")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn area_methods_route_to_project_subtree() {
        let (vfs, owner) = setup_vfs().await;
        let prefix = vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        let area = WritableArea::ProjectDir(prefix);

        vfs.area_write_bytes(&area, owner, "out.txt", b"data", None)
            .await
            .unwrap();
        // The file lands in the project's folder in global files.
        assert_eq!(
            vfs.read_global(owner, "Projects/Acme/out.txt")
                .await
                .unwrap(),
            "data"
        );
        // Listing the area root shows it relative to the project folder.
        let names: Vec<_> = vfs
            .area_list(&area, owner, "")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["out.txt".to_string()]);
    }

    #[tokio::test]
    async fn mounted_project_mode_only_project_dir_is_writable() {
        let (vfs, owner) = setup_vfs().await;
        let vfs = Arc::new(vfs);
        let prefix = vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        // A read-only personal file outside the project.
        vfs.write_global(owner, "personal/notes.txt", "secret")
            .await
            .unwrap();

        let fs = MountedVfs::new(vfs.clone(), owner, WritableArea::ProjectDir(prefix));
        // Writing inside the project folder works.
        fs.write_bytes("/Projects/Acme/out.txt", b"ok", None)
            .await
            .unwrap();
        assert_eq!(
            vfs.read_global(owner, "Projects/Acme/out.txt")
                .await
                .unwrap(),
            "ok"
        );
        // The rest of the namespace is readable but not writable.
        assert_eq!(
            fs.read_bytes("/personal/notes.txt").await.unwrap().0,
            b"secret"
        );
        assert!(
            fs.write_bytes("/personal/notes.txt", b"x", None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn mounted_extra_writable_dir_is_writable() {
        let (vfs, owner) = setup_vfs().await;
        let vfs = Arc::new(vfs);
        let prefix = vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        vfs.write_global(owner, "Documents/old.txt", "old")
            .await
            .unwrap();

        let fs = MountedVfs::new(vfs.clone(), owner, WritableArea::ProjectDir(prefix))
            .with_writable_dirs(vec!["Documents".to_string()]);

        // The configured directory and its children are writable.
        fs.write_bytes("/Documents/new.txt", b"hi", None)
            .await
            .unwrap();
        fs.write_bytes("/Documents/sub/deep.txt", b"deep", None)
            .await
            .unwrap();
        assert_eq!(
            vfs.read_global(owner, "Documents/sub/deep.txt")
                .await
                .unwrap(),
            "deep"
        );

        // A sibling outside the configured directory stays read-only.
        vfs.write_global(owner, "Other/x.txt", "ro").await.unwrap();
        assert!(fs.write_bytes("/Other/x.txt", b"no", None).await.is_err());
    }

    #[tokio::test]
    async fn mounted_workspace_mode_extra_dir_is_writable() {
        let (vfs, owner) = setup_vfs().await;
        let vfs = Arc::new(vfs);
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();

        let fs = MountedVfs::new(vfs.clone(), owner, WritableArea::Workspace(ws))
            .with_writable_dirs(vec!["Notes".to_string()]);

        // The workspace mount is writable as before.
        fs.write_bytes("/~workspace/a.txt", b"ws", None)
            .await
            .unwrap();
        // The extra global directory is writable too, routed to global files.
        fs.write_bytes("/Notes/todo.txt", b"todo", None)
            .await
            .unwrap();
        assert_eq!(
            vfs.read_global(owner, "Notes/todo.txt").await.unwrap(),
            "todo"
        );
        // Other global paths remain read-only.
        vfs.write_global(owner, "Secret/s.txt", "ro").await.unwrap();
        assert!(fs.write_bytes("/Secret/s.txt", b"no", None).await.is_err());
    }

    #[tokio::test]
    async fn staged_upload_round_trips_into_workspace() {
        let (vfs, owner) = setup_vfs().await;
        let staged = vfs
            .stage_upload(owner, "photo.png", Some("image/png"), b"bytes")
            .await
            .unwrap();
        assert_eq!(staged.name, "photo.png");
        assert_eq!(staged.size, 5);

        let taken = vfs.take_staged_upload(owner, staged.id).await.unwrap();
        assert_eq!(taken.name, "photo.png");
        assert_eq!(taken.mime_type.as_deref(), Some("image/png"));
        assert_eq!(taken.bytes, b"bytes");

        // A staged upload can be consumed only once.
        assert!(vfs.take_staged_upload(owner, staged.id).await.is_err());
    }

    #[tokio::test]
    async fn staged_upload_scoped_per_owner() {
        let (vfs, owner) = setup_vfs().await;
        let other = Uuid::now_v7();
        vfs.db
            .query_with_params(
                "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
                vec![
                    Value::Uuid(other),
                    Value::Text("bob".to_string()),
                    Value::Text("hash".to_string()),
                ],
            )
            .await
            .unwrap();
        let staged = vfs.stage_upload(owner, "a.txt", None, b"x").await.unwrap();
        // Another user cannot take someone else's staged upload.
        assert!(vfs.take_staged_upload(other, staged.id).await.is_err());
        // The owner still can.
        assert!(vfs.take_staged_upload(owner, staged.id).await.is_ok());
    }

    #[tokio::test]
    async fn cleanup_removes_only_stale_staged_uploads() {
        let (vfs, owner) = setup_vfs().await;
        let fresh = vfs
            .stage_upload(owner, "fresh.txt", None, b"f")
            .await
            .unwrap();
        let stale = vfs
            .stage_upload(owner, "stale.txt", None, b"s")
            .await
            .unwrap();
        // Backdate the stale upload's timestamp well past any cutoff.
        vfs.db
            .query_with_params(
                "UPDATE staged_uploads SET created_at = 0 WHERE id = ?",
                vec![Value::Uuid(stale.id)],
            )
            .await
            .unwrap();

        let removed = vfs.cleanup_staged_uploads(1).await.unwrap();
        assert_eq!(removed, 1);
        assert!(vfs.take_staged_upload(owner, stale.id).await.is_err());
        assert!(vfs.take_staged_upload(owner, fresh.id).await.is_ok());
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
