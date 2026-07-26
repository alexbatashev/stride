use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bashkit::{DirEntry, FileType, FsBackend, Metadata, Result};

use crate::vfs::{self, EntryKind, MountedVfs, VfsError};

/// Bashkit storage backend backed by the mounted VFS. Bashkit's `PosixFs`
/// wraps this to provide POSIX path semantics; here we only translate raw
/// storage operations onto the workspace/global namespace exposed by
/// [`MountedVfs`].
pub struct VfsBackend {
    fs: MountedVfs,
}

impl VfsBackend {
    pub fn new(fs: MountedVfs) -> Self {
        Self { fs }
    }

    /// Copies a file or directory tree from `from` to `to`.
    fn copy_tree<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let meta = self.stat(Path::new(from)).await?;
            if meta.file_type == FileType::Directory {
                self.fs.create_dir(to).await.map_err(map_err)?;
                for entry in self.fs.list(from).await.map_err(map_err)? {
                    let child_from = format!("{from}/{}", entry.name);
                    let child_to = format!("{to}/{}", entry.name);
                    self.copy_tree(&child_from, &child_to).await?;
                }
                Ok(())
            } else {
                let (bytes, mime) = self.fs.read_bytes(from).await.map_err(map_err)?;
                self.fs
                    .write_bytes(to, &bytes, mime.as_deref())
                    .await
                    .map_err(map_err)
            }
        })
    }
}

#[async_trait]
impl FsBackend for VfsBackend {
    async fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let (bytes, _) = self.fs.read_bytes(&to_str(path)).await.map_err(map_err)?;
        Ok(bytes)
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.fs
            .write_bytes(&to_str(path), content, None)
            .await
            .map_err(map_err)
    }

    async fn append(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = to_str(path);
        let mut data = self
            .fs
            .read_bytes(&path)
            .await
            .map(|(bytes, _)| bytes)
            .unwrap_or_default();
        data.extend_from_slice(content);
        self.fs
            .write_bytes(&path, &data, None)
            .await
            .map_err(map_err)
    }

    async fn mkdir(&self, path: &Path, _recursive: bool) -> Result<()> {
        self.fs.create_dir(&to_str(path)).await.map_err(map_err)
    }

    async fn remove(&self, path: &Path, _recursive: bool) -> Result<()> {
        self.fs.delete(&to_str(path)).await.map_err(map_err)
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = to_str(path);
        let segments = segments(&path);
        let Some((name, parents)) = segments.split_last() else {
            return Ok(dir_metadata(0, self.fs.stat_meta(&path, true).mode));
        };
        let entry = self
            .fs
            .list(&parents.join("/"))
            .await
            .map_err(map_err)?
            .into_iter()
            .find(|e| &e.name == name)
            .ok_or_else(|| not_found(&path))?;
        let is_dir = matches!(entry.kind, EntryKind::Directory);
        Ok(metadata_of(&entry, self.fs.stat_meta(&path, is_dir).mode))
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let base = to_str(path);
        let entries = self.fs.list(&base).await.map_err(map_err)?;
        Ok(entries
            .into_iter()
            .map(|e| {
                let is_dir = matches!(e.kind, EntryKind::Directory);
                let mode = self.fs.stat_meta(&join_path(&base, &e.name), is_dir).mode;
                DirEntry {
                    name: e.name.clone(),
                    metadata: metadata_of(&e, mode),
                }
            })
            .collect())
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        Ok(self.stat(path).await.is_ok())
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = to_str(from);
        self.copy_tree(&from, &to_str(to)).await?;
        self.fs.delete(&from).await.map_err(map_err)
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.copy_tree(&to_str(from), &to_str(to)).await
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> Result<()> {
        Err(unsupported("symbolic links"))
    }

    async fn read_link(&self, _path: &Path) -> Result<PathBuf> {
        Err(unsupported("symbolic links"))
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }

    async fn set_modified_time(&self, _path: &Path, _time: SystemTime) -> Result<()> {
        Ok(())
    }
}

fn to_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .map(str::to_string)
        .collect()
}

fn join_path(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

fn metadata_of(entry: &vfs::DirEntry, mode: u32) -> Metadata {
    let (file_type, size) = match entry.kind {
        EntryKind::Directory => (FileType::Directory, 0),
        EntryKind::File => (FileType::File, entry.size.unwrap_or(0).max(0) as u64),
    };
    Metadata {
        file_type,
        size,
        mode,
        modified: epoch(entry.updated_at),
        created: epoch(entry.updated_at),
    }
}

fn dir_metadata(updated_at: i64, mode: u32) -> Metadata {
    Metadata {
        file_type: FileType::Directory,
        size: 0,
        mode,
        modified: epoch(updated_at),
        created: epoch(updated_at),
    }
}

fn epoch(seconds: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds.max(0) as u64)
}

/// Translates a VFS error into a bashkit filesystem error, preserving the
/// read-only and not-found distinctions the shell surfaces to the user.
fn map_err(err: VfsError) -> bashkit::Error {
    let kind = match err {
        VfsError::ReadOnly | VfsError::PermissionDenied => ErrorKind::PermissionDenied,
        VfsError::NotFound => ErrorKind::NotFound,
        VfsError::NotADirectory | VfsError::IsADirectory | VfsError::Storage(_) => ErrorKind::Other,
    };
    std::io::Error::new(kind, err.to_string()).into()
}

fn not_found(path: &str) -> bashkit::Error {
    std::io::Error::new(
        ErrorKind::NotFound,
        format!("{path}: no such file or directory"),
    )
    .into()
}

fn unsupported(what: &str) -> bashkit::Error {
    std::io::Error::new(ErrorKind::Unsupported, format!("{what} are not supported")).into()
}
