use std::sync::Arc;

use anyhow::bail;
use uuid::Uuid;

use super::{DirEntry, EntryKind, Vfs};

/// Mount point under which a standalone thread workspace is exposed.
pub const WORKSPACE_MOUNT: &str = "~workspace";

/// Top-level global directory that holds one folder per project.
pub const PROJECTS_ROOT: &str = "Projects";

/// Where a thread's writable files live.
#[derive(Clone)]
pub enum WritableArea {
    /// A standalone workspace tree, exposed to the agent at `/~workspace`. The
    /// rest of `/` is the user's read-only global space.
    Workspace(Uuid),
    /// A writable subtree inside the user's global files, e.g. `Projects/Acme`.
    /// Everything else under `/` (including other projects) stays read-only.
    ProjectDir(String),
}

/// Presents a single namespace to the agent. For a [`WritableArea::Workspace`]
/// the writable tree is mounted at `/~workspace` next to the read-only global
/// files at `/`. For a [`WritableArea::ProjectDir`] the whole namespace is the
/// user's global space and only the project's folder is writable.
#[derive(Clone)]
pub struct MountedVfs {
    vfs: Arc<Vfs>,
    owner: Uuid,
    area: WritableArea,
    /// Extra writable global directories the user configured, as normalized
    /// prefixes. These and their descendants are writable on top of `area`.
    extra: Vec<String>,
}

enum Mount {
    /// Path inside the writable workspace (relative, no leading slash).
    Workspace(String),
    /// Path inside the global space (relative, no leading slash).
    Global(String),
}

/// A resolved writable destination, telling the caller which backend to use.
enum WriteTarget {
    Workspace(Uuid, String),
    Global(String),
}

impl MountedVfs {
    pub fn new(vfs: Arc<Vfs>, owner: Uuid, area: WritableArea) -> Self {
        Self {
            vfs,
            owner,
            area,
            extra: Vec::new(),
        }
    }

    /// Adds extra writable global directories (normalized prefixes).
    pub fn with_writable_dirs(mut self, dirs: Vec<String>) -> Self {
        self.extra = dirs;
        self
    }

    /// Normalizes an absolute or relative path into mount + relative path.
    fn resolve(&self, path: &str) -> Mount {
        let segments = clean_segments(path);
        match self.area {
            WritableArea::Workspace(_) => {
                if segments.first().map(String::as_str) == Some(WORKSPACE_MOUNT) {
                    Mount::Workspace(segments[1..].join("/"))
                } else {
                    Mount::Global(segments.join("/"))
                }
            }
            // In project mode the whole namespace is the global space; the
            // project folder is just an ordinary (writable) path within it.
            WritableArea::ProjectDir(_) => Mount::Global(segments.join("/")),
        }
    }

    /// Lists entries at `path`. In workspace mode the global root gets a
    /// synthetic `~workspace` directory entry; in project mode the project
    /// folder is a real global directory, so no synthesis is needed.
    pub async fn list(&self, path: &str) -> anyhow::Result<Vec<DirEntry>> {
        match self.resolve(path) {
            Mount::Workspace(rel) => self.vfs.list(self.workspace_id()?, &rel).await,
            Mount::Global(rel) => {
                let mut entries = self.vfs.list_global(self.owner, &rel).await?;
                if rel.is_empty() && matches!(self.area, WritableArea::Workspace(_)) {
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
            Mount::Workspace(rel) => self.vfs.read_bytes(self.workspace_id()?, &rel).await,
            Mount::Global(rel) => self.vfs.read_bytes_global(self.owner, &rel).await,
        }
    }

    pub async fn write_bytes(
        &self,
        path: &str,
        content: &[u8],
        mime_type: Option<&str>,
    ) -> anyhow::Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => {
                self.vfs
                    .write_bytes(id, &rel, content, mime_type, self.owner)
                    .await
            }
            WriteTarget::Global(rel) => {
                self.vfs
                    .write_bytes_global(self.owner, &rel, content, mime_type)
                    .await
            }
        }
    }

    pub async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => self.vfs.create_dir(id, &rel, self.owner).await,
            WriteTarget::Global(rel) => self.vfs.create_dir_global(self.owner, &rel).await,
        }
    }

    pub async fn delete(&self, path: &str) -> anyhow::Result<()> {
        match self.writable(path)? {
            WriteTarget::Workspace(id, rel) => self.vfs.delete(id, &rel).await,
            WriteTarget::Global(rel) => self.vfs.delete_global(self.owner, &rel).await,
        }
    }

    /// True when `rel` (a global path) falls inside a user-configured extra
    /// writable directory.
    fn extra_allows(&self, rel: &str) -> bool {
        self.extra
            .iter()
            .any(|prefix| rel == prefix || rel.starts_with(&format!("{prefix}/")))
    }

    /// Resolves `path` to a writable destination, rejecting read-only locations.
    fn writable(&self, path: &str) -> anyhow::Result<WriteTarget> {
        match (&self.area, self.resolve(path)) {
            (WritableArea::Workspace(id), Mount::Workspace(rel)) => {
                Ok(WriteTarget::Workspace(*id, rel))
            }
            (WritableArea::Workspace(_), Mount::Global(rel)) => {
                if self.extra_allows(&rel) {
                    Ok(WriteTarget::Global(rel))
                } else {
                    bail!("read-only: only /{WORKSPACE_MOUNT} is writable, {path} is not")
                }
            }
            (WritableArea::ProjectDir(prefix), Mount::Global(rel)) => {
                if rel == *prefix
                    || rel.starts_with(&format!("{prefix}/"))
                    || self.extra_allows(&rel)
                {
                    Ok(WriteTarget::Global(rel))
                } else {
                    bail!("read-only: only /{prefix} is writable, {path} is not")
                }
            }
            (WritableArea::ProjectDir(_), Mount::Workspace(_)) => unreachable!(),
        }
    }

    fn workspace_id(&self) -> anyhow::Result<Uuid> {
        match self.area {
            WritableArea::Workspace(id) => Ok(id),
            WritableArea::ProjectDir(_) => bail!("no workspace in project mode"),
        }
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
    use super::*;

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
}
