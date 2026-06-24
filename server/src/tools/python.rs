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
    /// Extra writable global directories, each mounted read-write at its real
    /// path on top of the read-only global tree.
    extra: Vec<ExtraMount>,
}

/// A user-configured writable global directory mounted into the sandbox.
struct ExtraMount {
    /// Normalized global prefix (no leading slash), e.g. `Documents/Notes`.
    prefix: String,
    /// Host mirror, mounted read-write at `/<prefix>`.
    host_dir: PathBuf,
}

impl VfsExecFileSystem {
    pub fn new(
        vfs: Arc<Vfs>,
        area: WritableArea,
        writable_extra: Vec<String>,
        owner: Uuid,
        host_dir: PathBuf,
    ) -> Self {
        let writable_root = match &area {
            WritableArea::Workspace(_) => format!("/{WORKSPACE_MOUNT}"),
            WritableArea::ProjectDir(prefix) => format!("/{prefix}"),
        };
        let extra = disjoint_extra(&area, writable_extra)
            .into_iter()
            .enumerate()
            .map(|(index, prefix)| ExtraMount {
                prefix,
                host_dir: host_dir.join(format!("rw-extra-{index}")),
            })
            .collect();
        Self {
            vfs,
            area,
            owner,
            global_dir: host_dir.join("root"),
            writable_dir: host_dir.join("rw"),
            writable_root,
            extra,
        }
    }
}

/// Drops extra directories that overlap the writable area or each other, so the
/// resulting read-write mounts never nest. Anything already writable through
/// the area, or contained in a broader extra directory, is removed.
fn disjoint_extra(area: &WritableArea, extra: Vec<String>) -> Vec<String> {
    let project_prefix = match area {
        WritableArea::ProjectDir(prefix) => Some(prefix.clone()),
        WritableArea::Workspace(_) => None,
    };
    let mut sorted: Vec<String> = extra.into_iter().filter(|p| !p.is_empty()).collect();
    sorted.sort();
    sorted.dedup();

    let mut kept: Vec<String> = Vec::new();
    for prefix in sorted {
        if let Some(project) = &project_prefix
            && (prefix == *project || is_under(&prefix, project) || is_under(project, &prefix))
        {
            continue;
        }
        if kept.iter().any(|k| prefix == *k || is_under(&prefix, k)) {
            continue;
        }
        kept.push(prefix);
    }
    kept
}

/// True when `path` is strictly inside `ancestor`.
fn is_under(path: &str, ancestor: &str) -> bool {
    path.starts_with(&format!("{ancestor}/"))
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

        // Each extra writable directory gets its own read-write mirror.
        for mount in &self.extra {
            let _ = tokio::fs::remove_dir_all(&mount.host_dir).await;
            tokio::fs::create_dir_all(&mount.host_dir).await?;
            sync_prefix_in(&self.vfs, self.owner, &mount.prefix, mount.host_dir.clone()).await?;
        }
        Ok(())
    }

    async fn after_execute(&self) -> anyhow::Result<()> {
        // Only the writable area is persisted; the read-only root is discarded.
        prune_area_missing(&self.vfs, &self.area, self.owner, self.writable_dir.clone()).await?;
        sync_area_out(&self.vfs, &self.area, self.owner, self.writable_dir.clone()).await?;

        for mount in &self.extra {
            prune_prefix_missing(&self.vfs, self.owner, &mount.prefix, mount.host_dir.clone())
                .await?;
            sync_prefix_out(&self.vfs, self.owner, &mount.prefix, mount.host_dir.clone()).await?;
        }
        Ok(())
    }

    fn volumes(&self) -> Vec<execenv::VolumeMount> {
        let mut volumes = vec![
            execenv::VolumeMount::read_only(&self.global_dir, "/"),
            execenv::VolumeMount::new(&self.writable_dir, &self.writable_root),
        ];
        for mount in &self.extra {
            let guest = format!("/{}", mount.prefix);
            volumes.push(execenv::VolumeMount::new(&mount.host_dir, &guest));
        }
        volumes
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

/// Copies a global subtree rooted at `prefix` into `host_dir`.
async fn sync_prefix_in(
    vfs: &Vfs,
    owner: Uuid,
    prefix: &str,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        tokio::fs::create_dir_all(host_dir.join(&rel)).await?;
        for entry in vfs.list_global(owner, &under(prefix, &rel)).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            match entry.kind {
                EntryKind::Directory => stack.push(child_rel),
                EntryKind::File => {
                    if let Some(parent) = child_local.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let (bytes, _) = vfs.read_bytes_global(owner, &under(prefix, &child_rel)).await?;
                    tokio::fs::write(child_local, bytes).await?;
                }
            }
        }
    }

    Ok(())
}

/// Deletes entries under `prefix` that the script removed from the host mirror.
async fn prune_prefix_missing(
    vfs: &Vfs,
    owner: Uuid,
    prefix: &str,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        for entry in vfs.list_global(owner, &under(prefix, &rel)).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            let local_type = tokio::fs::metadata(&child_local)
                .await
                .ok()
                .map(|m| (m.is_dir(), m.is_file()));

            match (entry.kind, local_type) {
                (_, None) => vfs.delete_global(owner, &under(prefix, &child_rel)).await?,
                (EntryKind::Directory, Some((true, _))) => stack.push(child_rel),
                (EntryKind::File, Some((_, true))) => {}
                _ => vfs.delete_global(owner, &under(prefix, &child_rel)).await?,
            }
        }
    }

    Ok(())
}

/// Writes the host mirror of `prefix` back into the global tree.
async fn sync_prefix_out(
    vfs: &Vfs,
    owner: Uuid,
    prefix: &str,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![(host_dir.clone(), String::new())];

    while let Some((local_dir, rel)) = stack.pop() {
        if !rel.is_empty() {
            vfs.create_dir_global(owner, &under(prefix, &rel)).await?;
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
                vfs.write_bytes_global(owner, &under(prefix, &child_rel), &bytes, None)
                    .await?;
            }
        }
    }

    Ok(())
}

/// Joins `rel` under a global `prefix`.
fn under(prefix: &str, rel: &str) -> String {
    if rel.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
}

fn join_rel(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_extra_drops_dirs_overlapping_project() {
        let area = WritableArea::ProjectDir("Projects/Acme".to_string());
        // Equal, descendant, and ancestor of the project dir are all dropped.
        let kept = disjoint_extra(
            &area,
            vec![
                "Projects/Acme".to_string(),
                "Projects/Acme/sub".to_string(),
                "Projects".to_string(),
                "Documents".to_string(),
            ],
        );
        assert_eq!(kept, vec!["Documents".to_string()]);
    }

    #[test]
    fn disjoint_extra_collapses_nested_and_duplicates() {
        let area = WritableArea::Workspace(Uuid::nil());
        let kept = disjoint_extra(
            &area,
            vec![
                "Notes".to_string(),
                "Notes".to_string(),
                "Notes/Personal".to_string(),
                "Photos".to_string(),
            ],
        );
        assert_eq!(kept, vec!["Notes".to_string(), "Photos".to_string()]);
    }
}
