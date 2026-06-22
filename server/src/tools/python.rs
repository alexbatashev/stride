use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use execenv::FileSystemBackend;
use uuid::Uuid;

use crate::vfs::{EntryKind, Vfs, WORKSPACE_MOUNT, WritableArea};

/// Presents the user's whole filesystem to the Python sandbox: the global files
/// are mounted read-only at `/`, and the thread's writable area is mounted
/// read-write at its real path on top. The guest resolves the nested writable
/// mount by longest-prefix, so the script can read every file but only write
/// inside the workspace or the project's folder.
pub struct VfsExecFileSystem {
    vfs: Arc<Vfs>,
    area: WritableArea,
    owner: Uuid,
    /// Host mirror of the read-only global tree, mounted at guest `/`.
    global_dir: PathBuf,
    /// Host mirror of the writable area, mounted read-write at `writable_root`.
    writable_dir: PathBuf,
    /// Absolute guest path of the writable area (e.g. `/~workspace`,
    /// `/Projects/Acme`).
    writable_root: String,
}

impl VfsExecFileSystem {
    pub fn new(vfs: Arc<Vfs>, area: WritableArea, owner: Uuid, host_dir: PathBuf) -> Self {
        let writable_root = match &area {
            WritableArea::Workspace(_) => format!("/{WORKSPACE_MOUNT}"),
            WritableArea::ProjectDir(prefix) => format!("/{prefix}"),
        };
        Self {
            vfs,
            area,
            owner,
            global_dir: host_dir.join("root"),
            writable_dir: host_dir.join("rw"),
            writable_root,
        }
    }
}

#[async_trait]
impl FileSystemBackend for VfsExecFileSystem {
    async fn before_execute(&self) -> anyhow::Result<()> {
        let _ = tokio::fs::remove_dir_all(&self.global_dir).await;
        let _ = tokio::fs::remove_dir_all(&self.writable_dir).await;
        tokio::fs::create_dir_all(&self.global_dir).await?;
        tokio::fs::create_dir_all(&self.writable_dir).await?;

        // The whole global tree, read-only, mounted at guest `/`.
        sync_global_in(&self.vfs, self.owner, self.global_dir.clone()).await?;
        // The writable area, read-write, mounted at its real path.
        sync_area_in(&self.vfs, &self.area, self.owner, self.writable_dir.clone()).await?;

        // A standalone workspace lives outside the global tree; add an empty
        // mount point so it still shows up when the script lists `/`.
        if matches!(self.area, WritableArea::Workspace(_)) {
            let rel = self.writable_root.trim_start_matches('/');
            tokio::fs::create_dir_all(self.global_dir.join(rel)).await?;
        }
        Ok(())
    }

    async fn after_execute(&self) -> anyhow::Result<()> {
        // Only the writable area is persisted; the read-only root is discarded.
        prune_area_missing(&self.vfs, &self.area, self.owner, self.writable_dir.clone()).await?;
        sync_area_out(&self.vfs, &self.area, self.owner, self.writable_dir.clone()).await
    }

    fn volumes(&self) -> Vec<execenv::VolumeMount> {
        vec![
            execenv::VolumeMount::read_only(&self.global_dir, "/"),
            execenv::VolumeMount::new(&self.writable_dir, &self.writable_root),
        ]
    }
}

/// Copies the user's global files into `host_dir` (read-only mirror).
async fn sync_global_in(vfs: &Vfs, owner: Uuid, host_dir: PathBuf) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        tokio::fs::create_dir_all(host_dir.join(&rel)).await?;
        for entry in vfs.list_global(owner, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            match entry.kind {
                EntryKind::Directory => stack.push(child_rel),
                EntryKind::File => {
                    if let Some(parent) = child_local.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let (bytes, _) = vfs.read_bytes_global(owner, &child_rel).await?;
                    tokio::fs::write(child_local, bytes).await?;
                }
            }
        }
    }

    Ok(())
}

/// Copies the writable area into `host_dir`.
async fn sync_area_in(
    vfs: &Vfs,
    area: &WritableArea,
    owner: Uuid,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        tokio::fs::create_dir_all(host_dir.join(&rel)).await?;
        for entry in vfs.area_list(area, owner, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            match entry.kind {
                EntryKind::Directory => stack.push(child_rel),
                EntryKind::File => {
                    if let Some(parent) = child_local.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let (bytes, _) = vfs.area_read_bytes(area, owner, &child_rel).await?;
                    tokio::fs::write(child_local, bytes).await?;
                }
            }
        }
    }

    Ok(())
}

/// Deletes area entries that the script removed from the host mirror.
async fn prune_area_missing(
    vfs: &Vfs,
    area: &WritableArea,
    owner: Uuid,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        for entry in vfs.area_list(area, owner, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            let local_type = tokio::fs::metadata(&child_local)
                .await
                .ok()
                .map(|m| (m.is_dir(), m.is_file()));

            match (entry.kind, local_type) {
                (_, None) => vfs.area_delete(area, owner, &child_rel).await?,
                (EntryKind::Directory, Some((true, _))) => stack.push(child_rel),
                (EntryKind::File, Some((_, true))) => {}
                _ => vfs.area_delete(area, owner, &child_rel).await?,
            }
        }
    }

    Ok(())
}

/// Writes the host mirror of the writable area back into the VFS.
async fn sync_area_out(
    vfs: &Vfs,
    area: &WritableArea,
    owner: Uuid,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![(host_dir.clone(), String::new())];

    while let Some((local_dir, rel)) = stack.pop() {
        if !rel.is_empty() {
            vfs.area_create_dir(area, owner, &rel).await?;
        }

        let mut entries = tokio::fs::read_dir(&local_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let child_rel = join_rel(&rel, &name);
            let child_local = entry.path();

            if file_type.is_dir() {
                stack.push((child_local, child_rel));
            } else if file_type.is_file() {
                let bytes = tokio::fs::read(child_local).await?;
                vfs.area_write_bytes(area, owner, &child_rel, &bytes, None)
                    .await?;
            }
        }
    }

    Ok(())
}

fn join_rel(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}
