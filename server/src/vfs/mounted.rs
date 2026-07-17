use std::fmt;
use std::sync::Arc;

use uuid::Uuid;

use super::{DirEntry, EntryKind, FileVersion, Vfs};

/// Absolute mount point of a thread's writable workspace tree.
pub const AGENT_HOME: &str = "/home/agent";

/// Absolute mount point of the user's global VFS tree.
pub const USER_HOME: &str = "/home/user";

/// Top-level global directory (relative to the user tree) holding one folder
/// per project.
pub const PROJECTS_ROOT: &str = "Projects";

/// Errno-style errors returned by the mount layer, replacing the previous
/// string-matched `read-only:`/`not found` contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    NotADirectory,
    IsADirectory,
    /// Write denied because the target is a read-only mount (`EROFS`).
    ReadOnly,
    /// Access denied for reasons other than a read-only mount (`EACCES`).
    /// Constructed by the sandbox layers in later stages.
    #[allow(dead_code)]
    PermissionDenied,
    /// Underlying storage failure carrying its message.
    Storage(String),
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VfsError::NotFound => write!(f, "no such file or directory"),
            VfsError::NotADirectory => write!(f, "not a directory"),
            VfsError::IsADirectory => write!(f, "is a directory"),
            VfsError::ReadOnly => write!(f, "read-only file system"),
            VfsError::PermissionDenied => write!(f, "permission denied"),
            VfsError::Storage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for VfsError {}

impl VfsError {
    /// Classifies an underlying storage error by inspecting its message.
    fn from_storage(err: anyhow::Error) -> Self {
        let message = err.to_string();
        if message.contains("not found") {
            VfsError::NotFound
        } else if message.contains("not a directory") {
            VfsError::NotADirectory
        } else if message.contains("is a directory") {
            VfsError::IsADirectory
        } else {
            VfsError::Storage(message)
        }
    }
}

pub type Result<T> = std::result::Result<T, VfsError>;

/// Where a resolved absolute path lands in the mount table.
enum Location {
    /// Inside the workspace tree; carries the relative path.
    Workspace(String),
    /// Inside the user's global tree; carries the relative path and whether it
    /// falls in a writable grant.
    Global { rel: String, writable: bool },
    /// The synthetic root `/`.
    Root,
    /// The synthetic `/home` directory.
    Home,
    /// A path handled by later sandbox layers (`/tmp`, `/usr/share`, ...); no
    /// VFS storage backs it at this layer.
    Outside,
}

/// Mount rules with no storage handle: resolves absolute paths to locations.
/// Kept separate so resolution is unit-testable without a live [`Vfs`].
#[derive(Clone)]
struct MountTable {
    /// Workspace mounted at `/home/agent`. `None` while a thread has no
    /// workspace yet (transitional; stage 4 makes every thread own one).
    workspace: Option<Uuid>,
    /// Relative global path of the project folder granted rw under `/home/user`.
    project_grant: Option<String>,
    /// Extra writable global directories (relative prefixes under `/home/user`).
    extra: Vec<String>,
}

impl MountTable {
    fn grant_allows(&self, rel: &str) -> bool {
        self.extra
            .iter()
            .chain(self.project_grant.iter())
            .any(|prefix| rel == prefix || rel.starts_with(&format!("{prefix}/")))
    }

    fn resolve(&self, path: &str) -> Location {
        let segments = clean_segments(path);
        match segments.split_first() {
            None => Location::Root,
            Some((first, rest)) if first == "home" => match rest.split_first() {
                None => Location::Home,
                Some((mount, tail)) if mount == "agent" => {
                    if self.workspace.is_some() {
                        Location::Workspace(tail.join("/"))
                    } else {
                        Location::Outside
                    }
                }
                Some((mount, tail)) if mount == "user" => {
                    let rel = tail.join("/");
                    let writable = self.grant_allows(&rel);
                    Location::Global { rel, writable }
                }
                _ => Location::Outside,
            },
            _ => Location::Outside,
        }
    }
}

/// Synthetic unix presentation metadata. No mode bits are stored; `ls -l`/`stat`
/// derive uid/gid/mode from the mount rules. The shell consumes `mode` (bashkit
/// renders only permission bits); `uid_name`/`gid_name` are surfaced by the
/// execenv presentation layer in stage 3.
pub(crate) mod stat {
    use super::{Location, MountTable};

    /// Stable numeric ids for the synthetic unix identities the agent sees.
    pub const UID_ROOT: u32 = 0;
    pub const GID_ROOT: u32 = 0;
    pub const UID_AGENT: u32 = 1000;
    pub const GID_AGENT: u32 = 1000;
    pub const UID_USER: u32 = 1001;
    pub const GID_USER: u32 = 1001;
    /// Shared group applied to granted (group-writable) subtrees.
    pub const GID_STRIDE: u32 = 1002;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StatMeta {
        /// Surfaced by the execenv presentation layer in stage 3.
        #[allow(dead_code)]
        pub uid: u32,
        /// Surfaced by the execenv presentation layer in stage 3.
        #[allow(dead_code)]
        pub gid: u32,
        pub mode: u32,
    }

    /// Maps a synthetic uid to its user name.
    #[allow(dead_code)]
    pub fn uid_name(uid: u32) -> &'static str {
        match uid {
            UID_AGENT => "agent",
            UID_USER => "user",
            _ => "root",
        }
    }

    /// Maps a synthetic gid to its group name.
    #[allow(dead_code)]
    pub fn gid_name(gid: u32) -> &'static str {
        match gid {
            GID_AGENT => "agent",
            GID_USER => "user",
            GID_STRIDE => "stride",
            _ => "root",
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Area {
        Agent,
        UserReadOnly,
        Granted,
        System,
    }

    impl Area {
        fn meta(self, is_dir: bool) -> StatMeta {
            match self {
                Area::Agent => StatMeta {
                    uid: UID_AGENT,
                    gid: GID_AGENT,
                    mode: if is_dir { 0o755 } else { 0o644 },
                },
                Area::UserReadOnly => StatMeta {
                    uid: UID_USER,
                    gid: GID_USER,
                    mode: if is_dir { 0o755 } else { 0o644 },
                },
                Area::Granted => StatMeta {
                    uid: UID_USER,
                    gid: GID_STRIDE,
                    mode: if is_dir { 0o775 } else { 0o664 },
                },
                Area::System => StatMeta {
                    uid: UID_ROOT,
                    gid: GID_ROOT,
                    mode: if is_dir { 0o755 } else { 0o644 },
                },
            }
        }
    }

    fn area_of(location: &Location) -> Area {
        match location {
            Location::Workspace(_) => Area::Agent,
            Location::Global { writable: true, .. } => Area::Granted,
            Location::Global {
                writable: false, ..
            } => Area::UserReadOnly,
            Location::Root | Location::Home | Location::Outside => Area::System,
        }
    }

    /// Synthetic uid/gid/mode for `path`, derived from mount rules only.
    pub(super) fn meta_for(table: &MountTable, path: &str, is_dir: bool) -> StatMeta {
        area_of(&table.resolve(path)).meta(is_dir)
    }
}

/// Presents a POSIX mount table to the agent: the thread workspace at
/// `/home/agent`, the user's global tree at `/home/user` (read-only by default),
/// with the project folder and configured directories writable on top.
#[derive(Clone)]
pub struct MountedVfs {
    vfs: Arc<Vfs>,
    owner: Uuid,
    table: MountTable,
}

impl MountedVfs {
    /// Builds a mount view for `workspace` at `/home/agent` plus an optional
    /// project grant under `/home/user`.
    pub fn new(
        vfs: Arc<Vfs>,
        owner: Uuid,
        workspace: Option<Uuid>,
        project_grant: Option<String>,
    ) -> Self {
        Self {
            vfs,
            owner,
            table: MountTable {
                workspace,
                project_grant,
                extra: Vec::new(),
            },
        }
    }

    /// Adds extra writable global directories (relative prefixes).
    pub fn with_writable_dirs(mut self, dirs: Vec<String>) -> Self {
        self.table.extra = dirs;
        self
    }

    /// Synthetic uid/gid/mode for `path`, derived from mount rules only.
    /// The shell's `ls -l`/`stat` consume the `mode` bits.
    pub fn stat_meta(&self, path: &str, is_dir: bool) -> stat::StatMeta {
        stat::meta_for(&self.table, path, is_dir)
    }

    pub async fn list(&self, path: &str) -> Result<Vec<DirEntry>> {
        match self.table.resolve(path) {
            Location::Root => Ok(vec![synthetic_dir("home")]),
            Location::Home => Ok(vec![synthetic_dir("agent"), synthetic_dir("user")]),
            Location::Workspace(rel) => self
                .vfs
                .list(self.workspace_id()?, &rel)
                .await
                .map_err(VfsError::from_storage),
            Location::Global { rel, .. } => self
                .vfs
                .list_global(self.owner, &rel)
                .await
                .map_err(VfsError::from_storage),
            Location::Outside => Err(VfsError::NotFound),
        }
    }

    pub async fn read_bytes(&self, path: &str) -> Result<(Vec<u8>, Option<String>)> {
        match self.table.resolve(path) {
            Location::Root | Location::Home => Err(VfsError::IsADirectory),
            Location::Workspace(rel) => self
                .vfs
                .read_bytes(self.workspace_id()?, &rel)
                .await
                .map_err(VfsError::from_storage),
            Location::Global { rel, .. } => self
                .vfs
                .read_bytes_global(self.owner, &rel)
                .await
                .map_err(VfsError::from_storage),
            Location::Outside => Err(VfsError::NotFound),
        }
    }

    pub async fn write_bytes(
        &self,
        path: &str,
        content: &[u8],
        mime_type: Option<&str>,
    ) -> Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => self
                .vfs
                .write_bytes(id, &rel, content, mime_type, self.owner)
                .await
                .map_err(VfsError::from_storage),
            WriteTarget::Global(rel) => self
                .vfs
                .write_bytes_global(self.owner, &rel, content, mime_type)
                .await
                .map_err(VfsError::from_storage),
        }
    }

    pub async fn list_versions(&self, path: &str) -> Result<Vec<FileVersion>> {
        match self.table.resolve(path) {
            Location::Root | Location::Home => Err(VfsError::IsADirectory),
            Location::Workspace(rel) => self
                .vfs
                .list_versions(self.workspace_id()?, &rel)
                .await
                .map_err(VfsError::from_storage),
            Location::Global { rel, .. } => self
                .vfs
                .list_versions_global(self.owner, &rel)
                .await
                .map_err(VfsError::from_storage),
            Location::Outside => Err(VfsError::NotFound),
        }
    }

    pub async fn read_version(
        &self,
        path: &str,
        version: i64,
    ) -> Result<(Vec<u8>, Option<String>)> {
        match self.table.resolve(path) {
            Location::Root | Location::Home => Err(VfsError::IsADirectory),
            Location::Workspace(rel) => self
                .vfs
                .read_version(self.workspace_id()?, &rel, version)
                .await
                .map_err(VfsError::from_storage),
            Location::Global { rel, .. } => self
                .vfs
                .read_version_global(self.owner, &rel, version)
                .await
                .map_err(VfsError::from_storage),
            Location::Outside => Err(VfsError::NotFound),
        }
    }

    pub async fn restore_version(&self, path: &str, version: i64) -> Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => self
                .vfs
                .restore_version(id, &rel, version)
                .await
                .map_err(VfsError::from_storage),
            WriteTarget::Global(rel) => self
                .vfs
                .restore_version_global(self.owner, &rel, version)
                .await
                .map_err(VfsError::from_storage),
        }
    }

    pub async fn create_dir(&self, path: &str) -> Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => self
                .vfs
                .create_dir(id, &rel, self.owner)
                .await
                .map_err(VfsError::from_storage),
            WriteTarget::Global(rel) => self
                .vfs
                .create_dir_global(self.owner, &rel)
                .await
                .map_err(VfsError::from_storage),
        }
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => self
                .vfs
                .delete(id, &rel)
                .await
                .map_err(VfsError::from_storage),
            WriteTarget::Global(rel) => self
                .vfs
                .delete_global(self.owner, &rel)
                .await
                .map_err(VfsError::from_storage),
        }
    }

    fn writable(&self, path: &str) -> Result<WriteTarget> {
        match self.table.resolve(path) {
            Location::Workspace(rel) => Ok(WriteTarget::Workspace(self.workspace_id()?, rel)),
            Location::Global {
                rel,
                writable: true,
            } => Ok(WriteTarget::Global(rel)),
            Location::Global {
                writable: false, ..
            }
            | Location::Root
            | Location::Home => Err(VfsError::ReadOnly),
            Location::Outside => Err(VfsError::NotFound),
        }
    }

    fn workspace_id(&self) -> Result<Uuid> {
        self.table.workspace.ok_or(VfsError::NotFound)
    }
}

/// A resolved writable destination, telling the caller which backend to use.
enum WriteTarget {
    Workspace(Uuid, String),
    Global(String),
}

fn synthetic_dir(name: &str) -> DirEntry {
    DirEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        size: None,
        updated_at: 0,
        mime_type: None,
    }
}

fn clean_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .map(str::to_string)
        .collect()
}

/// Builds the global path of a project's folder from its title.
pub fn project_dir_prefix(title: &str) -> String {
    format!("{PROJECTS_ROOT}/{}", project_dir_name(title))
}

/// Sanitizes a project title into a single safe path segment.
pub fn project_dir_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_matches('.').trim();
    if name.is_empty() {
        "Untitled".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::stat::{GID_AGENT, meta_for};
    use super::stat::{
        GID_ROOT, GID_STRIDE, GID_USER, StatMeta, UID_AGENT, UID_ROOT, UID_USER, gid_name, uid_name,
    };
    use super::*;

    fn table(workspace: Option<Uuid>, grant: Option<&str>, extra: &[&str]) -> MountTable {
        MountTable {
            workspace,
            project_grant: grant.map(str::to_string),
            extra: extra.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn meta(t: &MountTable, path: &str, is_dir: bool) -> StatMeta {
        meta_for(t, path, is_dir)
    }

    #[test]
    fn project_dir_name_sanitizes_separators() {
        assert_eq!(project_dir_name("Acme"), "Acme");
        assert_eq!(project_dir_name("a/b\\c"), "a-b-c");
        assert_eq!(project_dir_name("  spaced  "), "spaced");
    }

    #[test]
    fn project_dir_name_falls_back_when_empty() {
        assert_eq!(project_dir_name("   "), "Untitled");
        assert_eq!(project_dir_name("..."), "Untitled");
    }

    #[test]
    fn project_dir_prefix_is_under_projects_root() {
        assert_eq!(project_dir_prefix("Acme"), "Projects/Acme");
    }

    #[test]
    fn stat_meta_agent_subtree_is_agent_owned() {
        let t = table(Some(Uuid::nil()), None, &[]);
        assert_eq!(
            meta(&t, "/home/agent/notes", true),
            StatMeta {
                uid: UID_AGENT,
                gid: GID_AGENT,
                mode: 0o755,
            }
        );
        assert_eq!(
            meta(&t, "/home/agent/notes/a.txt", false),
            StatMeta {
                uid: UID_AGENT,
                gid: GID_AGENT,
                mode: 0o644,
            }
        );
    }

    #[test]
    fn stat_meta_user_default_is_read_only_user_owned() {
        let t = table(Some(Uuid::nil()), None, &[]);
        assert_eq!(
            meta(&t, "/home/user/Documents", true),
            StatMeta {
                uid: UID_USER,
                gid: GID_USER,
                mode: 0o755,
            }
        );
        assert_eq!(
            meta(&t, "/home/user/Documents/a.txt", false),
            StatMeta {
                uid: UID_USER,
                gid: GID_USER,
                mode: 0o644,
            }
        );
    }

    #[test]
    fn stat_meta_granted_subtree_is_group_writable() {
        let t = table(Some(Uuid::nil()), Some("Projects/Acme"), &["Documents"]);
        assert_eq!(
            meta(&t, "/home/user/Projects/Acme", true),
            StatMeta {
                uid: UID_USER,
                gid: GID_STRIDE,
                mode: 0o775,
            }
        );
        assert_eq!(
            meta(&t, "/home/user/Projects/Acme/out.txt", false),
            StatMeta {
                uid: UID_USER,
                gid: GID_STRIDE,
                mode: 0o664,
            }
        );
        assert_eq!(meta(&t, "/home/user/Documents/x", false).gid, GID_STRIDE);
        // A sibling project stays read-only.
        assert_eq!(meta(&t, "/home/user/Projects/Other", true).gid, GID_USER);
    }

    #[test]
    fn stat_meta_synthetic_and_outside_are_root() {
        let t = table(Some(Uuid::nil()), None, &[]);
        for path in ["/", "/home", "/tmp/scratch", "/usr/share/fonts"] {
            let m = meta(&t, path, true);
            assert_eq!(m.uid, UID_ROOT);
            assert_eq!(m.gid, GID_ROOT);
        }
    }

    #[test]
    fn resolve_agent_without_workspace_is_outside() {
        let t = table(None, Some("Projects/Acme"), &[]);
        assert!(matches!(t.resolve("/home/agent/x.txt"), Location::Outside));
        assert!(matches!(
            t.resolve("/home/user/Projects/Acme/out.txt"),
            Location::Global { writable: true, .. }
        ));
    }

    #[test]
    fn identity_names_map_to_stable_labels() {
        assert_eq!(uid_name(UID_AGENT), "agent");
        assert_eq!(uid_name(UID_USER), "user");
        assert_eq!(uid_name(UID_ROOT), "root");
        assert_eq!(gid_name(GID_STRIDE), "stride");
        assert_eq!(gid_name(GID_ROOT), "root");
    }
}
