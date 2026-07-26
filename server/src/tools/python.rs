use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use execenv::FileSystemBackend;
use uuid::Uuid;

use crate::vfs::{AGENT_HOME, EntryKind, USER_HOME, Vfs};

/// Presents the POSIX guest layout to the Python sandbox: the user's global tree
/// is mounted read-only at `/home/user`, the thread workspace is mounted
/// read-write at `/home/agent`, and each writable grant (the project folder plus
/// user-configured directories) is mounted read-write at its `/home/user/<prefix>`
/// path on top of the read-only tree. `/tmp` scratch and `/usr/share` runtime
/// assets are supplied by the execenv sandbox layer, not here.
pub struct VfsExecFileSystem {
    vfs: Arc<Vfs>,
    owner: Uuid,
    workspace: Option<Uuid>,
    /// Host mirror of the read-only global tree, mounted at `/home/user`.
    global_dir: PathBuf,
    /// Host mirror of the workspace, mounted read-write at `/home/agent`.
    workspace_dir: PathBuf,
    /// Writable global grants, each mounted read-write at `/home/user/<prefix>`.
    grants: Vec<GrantMount>,
}

/// A writable global subtree mounted into the sandbox on top of `/home/user`.
struct GrantMount {
    /// Normalized global prefix (no leading slash), e.g. `Projects/Acme`.
    prefix: String,
    /// Host mirror, mounted read-write at `/home/user/<prefix>`.
    host_dir: PathBuf,
}

impl VfsExecFileSystem {
    pub fn new(
        vfs: Arc<Vfs>,
        owner: Uuid,
        workspace: Option<Uuid>,
        project_grant: Option<String>,
        writable_extra: Vec<String>,
        host_dir: PathBuf,
    ) -> Self {
        let mut prefixes = writable_extra;
        if let Some(prefix) = project_grant {
            prefixes.push(prefix);
        }
        let grants = disjoint_prefixes(prefixes)
            .into_iter()
            .enumerate()
            .map(|(index, prefix)| GrantMount {
                prefix,
                host_dir: host_dir.join(format!("rw-grant-{index}")),
            })
            .collect();
        Self {
            vfs,
            owner,
            workspace,
            global_dir: host_dir.join("user"),
            workspace_dir: host_dir.join("agent"),
            grants,
        }
    }
}

/// Sorts, deduplicates and drops any prefix nested in another, so the resulting
/// read-write grant mounts never overlap.
fn disjoint_prefixes(prefixes: Vec<String>) -> Vec<String> {
    let mut sorted: Vec<String> = prefixes.into_iter().filter(|p| !p.is_empty()).collect();
    sorted.sort();
    sorted.dedup();

    let mut kept: Vec<String> = Vec::new();
    for prefix in sorted {
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
        tokio::fs::create_dir_all(&self.global_dir).await?;
        sync_global_in(&self.vfs, self.owner, self.global_dir.clone()).await?;

        if let Some(id) = self.workspace {
            let _ = tokio::fs::remove_dir_all(&self.workspace_dir).await;
            tokio::fs::create_dir_all(&self.workspace_dir).await?;
            sync_workspace_in(&self.vfs, id, self.workspace_dir.clone()).await?;
        }

        for mount in &self.grants {
            let _ = tokio::fs::remove_dir_all(&mount.host_dir).await;
            tokio::fs::create_dir_all(&mount.host_dir).await?;
            sync_prefix_in(&self.vfs, self.owner, &mount.prefix, mount.host_dir.clone()).await?;
        }
        Ok(())
    }

    async fn after_execute(&self) -> anyhow::Result<()> {
        // The read-only `/home/user` mirror is discarded; only the workspace and
        // the writable grants are persisted back into the VFS.
        if let Some(id) = self.workspace {
            prune_workspace_missing(&self.vfs, id, self.workspace_dir.clone()).await?;
            sync_workspace_out(&self.vfs, id, self.owner, self.workspace_dir.clone()).await?;
        }

        for mount in &self.grants {
            prune_prefix_missing(&self.vfs, self.owner, &mount.prefix, mount.host_dir.clone())
                .await?;
            sync_prefix_out(&self.vfs, self.owner, &mount.prefix, mount.host_dir.clone()).await?;
        }
        Ok(())
    }

    fn volumes(&self) -> Vec<execenv::VolumeMount> {
        let mut volumes = vec![execenv::VolumeMount::read_only(&self.global_dir, USER_HOME)];
        if self.workspace.is_some() {
            volumes.push(execenv::VolumeMount::new(&self.workspace_dir, AGENT_HOME));
        }
        for mount in &self.grants {
            let guest = format!("{USER_HOME}/{}", mount.prefix);
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

/// Copies the workspace into `host_dir`.
async fn sync_workspace_in(vfs: &Vfs, workspace: Uuid, host_dir: PathBuf) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        tokio::fs::create_dir_all(host_dir.join(&rel)).await?;
        for entry in vfs.list(workspace, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            match entry.kind {
                EntryKind::Directory => stack.push(child_rel),
                EntryKind::File => {
                    if let Some(parent) = child_local.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let (bytes, _) = vfs.read_bytes(workspace, &child_rel).await?;
                    tokio::fs::write(child_local, bytes).await?;
                }
            }
        }
    }

    Ok(())
}

/// Deletes workspace entries that the script removed from the host mirror.
async fn prune_workspace_missing(
    vfs: &Vfs,
    workspace: Uuid,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![String::new()];

    while let Some(rel) = stack.pop() {
        for entry in vfs.list(workspace, &rel).await? {
            let child_rel = join_rel(&rel, &entry.name);
            let child_local = host_dir.join(&child_rel);
            let local_type = tokio::fs::metadata(&child_local)
                .await
                .ok()
                .map(|m| (m.is_dir(), m.is_file()));

            match (entry.kind, local_type) {
                (_, None) => vfs.delete(workspace, &child_rel).await?,
                (EntryKind::Directory, Some((true, _))) => stack.push(child_rel),
                (EntryKind::File, Some((_, true))) => {}
                _ => vfs.delete(workspace, &child_rel).await?,
            }
        }
    }

    Ok(())
}

/// Writes the host mirror of the workspace back into the VFS.
async fn sync_workspace_out(
    vfs: &Vfs,
    workspace: Uuid,
    owner: Uuid,
    host_dir: PathBuf,
) -> anyhow::Result<()> {
    let mut stack = vec![(host_dir.clone(), String::new())];

    while let Some((local_dir, rel)) = stack.pop() {
        if !rel.is_empty() {
            vfs.create_dir(workspace, &rel, owner).await?;
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
                vfs.write_bytes(workspace, &child_rel, &bytes, None, owner)
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
                    let (bytes, _) = vfs
                        .read_bytes_global(owner, &under(prefix, &child_rel))
                        .await?;
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
    use std::sync::Arc;

    use minisql::{ConnectionPool, Value};
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::db;
    use crate::vfs::{AnyFileProvider, LocalFileProvider};

    async fn setup_vfs() -> (Arc<Vfs>, Uuid) {
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
        let storage = AnyFileProvider::Local(
            LocalFileProvider::with_id_gen(base, Arc::new(stride_agent::SystemIdGen)).unwrap(),
        );
        let vfs = Vfs::with_clock(
            db,
            storage,
            3,
            Arc::new(stride_agent::SystemClock),
            Arc::new(stride_agent::SystemIdGen),
        );
        (Arc::new(vfs), owner)
    }

    fn volume_at<'a>(
        volumes: &'a [execenv::VolumeMount],
        guest: &str,
    ) -> Option<&'a execenv::VolumeMount> {
        volumes.iter().find(|v| v.guest_path == guest)
    }

    #[test]
    fn disjoint_prefixes_drops_nested_and_duplicates() {
        let kept = disjoint_prefixes(vec![
            "Projects/Acme".to_string(),
            "Projects/Acme".to_string(),
            "Projects/Acme/sub".to_string(),
            "Documents".to_string(),
            "Documents/Notes".to_string(),
        ]);
        assert_eq!(
            kept,
            vec!["Documents".to_string(), "Projects/Acme".to_string()]
        );
    }

    #[test]
    fn disjoint_prefixes_drops_empty() {
        let kept = disjoint_prefixes(vec!["".to_string(), "Photos".to_string()]);
        assert_eq!(kept, vec!["Photos".to_string()]);
    }

    #[tokio::test]
    async fn volumes_mount_user_ro_agent_rw_and_omit_synthetic_paths() {
        let (vfs, owner) = setup_vfs().await;
        let workspace = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        let dir = TempDir::new().unwrap();
        let fs = VfsExecFileSystem::new(
            vfs,
            owner,
            Some(workspace),
            None,
            Vec::new(),
            dir.path().to_path_buf(),
        );

        let volumes = fs.volumes();
        let user = volume_at(&volumes, USER_HOME).expect("user mount");
        assert!(user.read_only, "/home/user must be read-only");
        let agent = volume_at(&volumes, AGENT_HOME).expect("agent mount");
        assert!(!agent.read_only, "/home/agent must be writable");

        assert!(volume_at(&volumes, "/").is_none(), "nothing at bare /");
        assert!(
            volume_at(&volumes, "/tmp").is_none(),
            "no /tmp from the vfs"
        );
        assert!(
            volumes.iter().all(|v| !v.guest_path.starts_with("/usr")),
            "no /usr/share from the vfs"
        );
    }

    #[tokio::test]
    async fn grant_mounts_are_writable_under_user() {
        let (vfs, owner) = setup_vfs().await;
        let prefix = vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        let dir = TempDir::new().unwrap();
        let fs = VfsExecFileSystem::new(
            vfs,
            owner,
            None,
            Some(prefix.clone()),
            vec!["Documents".to_string()],
            dir.path().to_path_buf(),
        );

        let volumes = fs.volumes();
        assert!(volume_at(&volumes, AGENT_HOME).is_none(), "no workspace");

        let project = volume_at(&volumes, &format!("{USER_HOME}/{prefix}")).expect("project grant");
        assert!(!project.read_only);
        let docs = volume_at(&volumes, &format!("{USER_HOME}/Documents")).expect("documents grant");
        assert!(!docs.read_only);
    }

    #[tokio::test]
    async fn workspace_round_trip_persists_only_writable_tree() {
        let (vfs, owner) = setup_vfs().await;
        let workspace = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        vfs.write(workspace, "seed.txt", "seed", owner)
            .await
            .unwrap();
        vfs.write_global(owner, "readme.txt", "global")
            .await
            .unwrap();

        let dir = TempDir::new().unwrap();
        let fs = VfsExecFileSystem::new(
            vfs.clone(),
            owner,
            Some(workspace),
            None,
            Vec::new(),
            dir.path().to_path_buf(),
        );

        fs.before_execute().await.unwrap();
        // The script writes a new file into /home/agent and tampers with the
        // read-only /home/user mirror.
        tokio::fs::write(fs.workspace_dir.join("out.txt"), b"produced")
            .await
            .unwrap();
        tokio::fs::write(fs.global_dir.join("readme.txt"), b"tampered")
            .await
            .unwrap();
        fs.after_execute().await.unwrap();

        assert_eq!(vfs.read(workspace, "out.txt").await.unwrap(), "produced");
        // The read-only tree is discarded: the tampered write never persists.
        assert_eq!(
            vfs.read_global(owner, "readme.txt").await.unwrap(),
            "global"
        );
    }

    #[tokio::test]
    async fn grant_subtree_round_trips_through_sync() {
        let (vfs, owner) = setup_vfs().await;
        let prefix = vfs.ensure_project_dir(owner, "Acme").await.unwrap();
        vfs.write_global(owner, "Documents/keep.txt", "keep")
            .await
            .unwrap();

        let dir = TempDir::new().unwrap();
        let fs = VfsExecFileSystem::new(
            vfs.clone(),
            owner,
            None,
            Some(prefix.clone()),
            Vec::new(),
            dir.path().to_path_buf(),
        );

        fs.before_execute().await.unwrap();
        let grant = &fs.grants[0];
        tokio::fs::write(grant.host_dir.join("report.txt"), b"result")
            .await
            .unwrap();
        fs.after_execute().await.unwrap();

        assert_eq!(
            vfs.read_global(owner, "Projects/Acme/report.txt")
                .await
                .unwrap(),
            "result"
        );
        // A read-only global file outside the grant is untouched.
        assert_eq!(
            vfs.read_global(owner, "Documents/keep.txt").await.unwrap(),
            "keep"
        );
    }
}
