use std::collections::HashSet;
use std::sync::Mutex;
use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use execenv::FileSystemBackend;
use uuid::Uuid;

use crate::vfs::{EntryKind, Vfs};

pub struct VfsExecFileSystem {
    vfs: Arc<Vfs>,
    workspace_id: Uuid,
    owner: Uuid,
    host_dir: PathBuf,
    /// Host-relative paths populated from the read-only global space. They are
    /// excluded from the write-back so global files stay read-only.
    global_paths: Mutex<HashSet<String>>,
}

impl VfsExecFileSystem {
    pub fn new(vfs: Arc<Vfs>, workspace_id: Uuid, owner: Uuid, host_dir: PathBuf) -> Self {
        Self {
            vfs,
            workspace_id,
            owner,
            host_dir,
            global_paths: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl FileSystemBackend for VfsExecFileSystem {
    async fn before_execute(&self) -> anyhow::Result<()> {
        let _ = tokio::fs::remove_dir_all(&self.host_dir).await;
        tokio::fs::create_dir_all(&self.host_dir).await?;
        sync_from_vfs(self.vfs.clone(), self.workspace_id, self.host_dir.clone()).await?;
        let global = sync_global_in(self.vfs.clone(), self.owner, self.host_dir.clone()).await?;
        *self.global_paths.lock().unwrap() = global;
        Ok(())
    }

    async fn after_execute(&self) -> anyhow::Result<()> {
        let global = self.global_paths.lock().unwrap().clone();
        prune_vfs_missing(
            self.vfs.clone(),
            self.workspace_id,
            self.host_dir.clone(),
            String::new(),
        )
        .await?;
        sync_to_vfs(
            self.vfs.clone(),
            self.workspace_id,
            self.owner,
            self.host_dir.clone(),
            &global,
        )
        .await
    }

    fn volumes(&self) -> Vec<execenv::VolumeMount> {
        let guest = format!("/{}", crate::vfs::WORKSPACE_MOUNT);
        vec![execenv::VolumeMount::new(&self.host_dir, guest)]
    }
}

async fn sync_from_vfs(vfs: Arc<Vfs>, workspace_id: Uuid, host_dir: PathBuf) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        let local_dir = host_dir.join(&rel);
        tokio::fs::create_dir_all(&local_dir).await?;

        for entry in vfs.list(workspace_id, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            match entry.kind {
                EntryKind::Directory => {
                    tokio::fs::create_dir_all(&child_local).await?;
                    stack.push(child_rel);
                }
                EntryKind::File => {
                    if let Some(parent) = child_local.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let (bytes, _) = vfs.read_bytes(workspace_id, &child_rel).await?;
                    tokio::fs::write(child_local, bytes).await?;
                }
            }
        }
    }

    Ok(())
}

/// Copies the user's read-only global files into the host directory and returns
/// the set of host-relative paths that came from the global space (skipping any
/// path already provided by the writable workspace).
async fn sync_global_in(
    vfs: Arc<Vfs>,
    owner: Uuid,
    host_dir: PathBuf,
) -> anyhow::Result<HashSet<String>> {
    let mut global = HashSet::new();
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        for entry in vfs.list_global(owner, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            if child_local.exists() {
                // Workspace file shadows the global one; keep it writable.
                if matches!(entry.kind, EntryKind::Directory) {
                    stack.push(child_rel);
                }
                continue;
            }
            match entry.kind {
                EntryKind::Directory => {
                    tokio::fs::create_dir_all(&child_local).await?;
                    global.insert(child_rel.clone());
                    stack.push(child_rel);
                }
                EntryKind::File => {
                    if let Some(parent) = child_local.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let (bytes, _) = vfs.read_bytes_global(owner, &child_rel).await?;
                    tokio::fs::write(child_local, bytes).await?;
                    global.insert(child_rel);
                }
            }
        }
    }

    Ok(global)
}

async fn prune_vfs_missing(
    vfs: Arc<Vfs>,
    workspace_id: Uuid,
    host_dir: PathBuf,
    root_rel: String,
) -> anyhow::Result<()> {
    let mut stack = vec![root_rel];

    while let Some(rel) = stack.pop() {
        for entry in vfs.list(workspace_id, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            let local_type = tokio::fs::metadata(&child_local)
                .await
                .ok()
                .map(|m| (m.is_dir(), m.is_file()));

            match (entry.kind, local_type) {
                (_, None) => vfs.delete(workspace_id, &child_rel).await?,
                (EntryKind::Directory, Some((true, _))) => stack.push(child_rel),
                (EntryKind::File, Some((_, true))) => {}
                _ => vfs.delete(workspace_id, &child_rel).await?,
            }
        }
    }

    Ok(())
}

async fn sync_to_vfs(
    vfs: Arc<Vfs>,
    workspace_id: Uuid,
    owner: Uuid,
    host_dir: PathBuf,
    global: &HashSet<String>,
) -> anyhow::Result<()> {
    let mut stack = vec![(host_dir.clone(), String::new())];

    while let Some((local_dir, rel)) = stack.pop() {
        if !rel.is_empty() && !global.contains(&rel) {
            vfs.create_dir(workspace_id, &rel, owner).await?;
        }

        let mut entries = tokio::fs::read_dir(&local_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let child_rel = join_rel(&rel, &name);
            if global.contains(&child_rel) {
                continue;
            }
            let child_local = entry.path();

            if file_type.is_dir() {
                stack.push((child_local, child_rel));
            } else if file_type.is_file() {
                let bytes = tokio::fs::read(child_local).await?;
                vfs.write_bytes(workspace_id, &child_rel, &bytes, None, owner)
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
