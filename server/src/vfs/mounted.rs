use std::sync::Arc;

use anyhow::bail;
use uuid::Uuid;

use super::{DirEntry, EntryKind, Vfs};

/// Mount point under which the writable thread/project workspace is exposed.
pub const WORKSPACE_MOUNT: &str = "~workspace";

/// Presents a single namespace to the agent: the writable thread/project
/// workspace mounted at `/~workspace`, and the user's read-only global files
/// alongside it under `/`.
#[derive(Clone)]
pub struct MountedVfs {
    vfs: Arc<Vfs>,
    workspace_id: Uuid,
    owner: Uuid,
}

enum Mount {
    /// Path inside the writable workspace (relative, no leading slash).
    Workspace(String),
    /// Path inside the read-only global space (relative, no leading slash).
    Global(String),
}

impl MountedVfs {
    pub fn new(vfs: Arc<Vfs>, workspace_id: Uuid, owner: Uuid) -> Self {
        Self {
            vfs,
            workspace_id,
            owner,
        }
    }

    /// Normalizes an absolute or relative path into mount + relative path.
    /// The first `~workspace` segment selects the writable workspace; anything
    /// else lives in the read-only global space.
    fn resolve(&self, path: &str) -> Mount {
        let segments: Vec<&str> = path
            .split('/')
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .collect();

        if segments.first() == Some(&WORKSPACE_MOUNT) {
            Mount::Workspace(segments[1..].join("/"))
        } else {
            Mount::Global(segments.join("/"))
        }
    }

    /// Lists entries at `path`. The root merges global files with a synthetic
    /// `~workspace` directory entry.
    pub async fn list(&self, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        match self.resolve(path) {
            Mount::Workspace(rel) => self.vfs.list(self.workspace_id, &rel).await,
            Mount::Global(rel) => {
                let mut entries = self.vfs.list_global(self.owner, &rel).await?;
                if rel.is_empty() {
                    entries.insert(
                        0,
                        DirEntry {
                            name: WORKSPACE_MOUNT.to_string(),
                            kind: EntryKind::Directory,
                            size: None,
                            updated_at: 0,
                            mime_type: None,
                        },
                    );
                }
                Ok(entries)
            }
        }
    }

    pub async fn read_bytes(&self, path: &str) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        match self.resolve(path) {
            Mount::Workspace(rel) => self.vfs.read_bytes(self.workspace_id, &rel).await,
            Mount::Global(rel) => self.vfs.read_bytes_global(self.owner, &rel).await,
        }
    }

    pub async fn write_bytes(
        &self,
        path: &str,
        content: &[u8],
        mime_type: Option<&str>,
    ) -> anyhow::Result<()> {
        let rel = self.writable(path)?;
        self.vfs
            .write_bytes(self.workspace_id, &rel, content, mime_type, self.owner)
            .await
    }

    pub async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let rel = self.writable(path)?;
        self.vfs
            .create_dir(self.workspace_id, &rel, self.owner)
            .await
    }

    pub async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let rel = self.writable(path)?;
        self.vfs.delete(self.workspace_id, &rel).await
    }

    /// Resolves `path` to a workspace-relative path, rejecting read-only global
    /// locations.
    fn writable(&self, path: &str) -> anyhow::Result<String> {
        match self.resolve(path) {
            Mount::Workspace(rel) => Ok(rel),
            Mount::Global(_) => {
                bail!("read-only: only /{WORKSPACE_MOUNT} is writable, {path} is not")
            }
        }
    }
}
