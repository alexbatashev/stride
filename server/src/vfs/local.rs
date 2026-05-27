use std::path::PathBuf;

use anyhow::Context;
use uuid::Uuid;

use super::FileProvider;

/// Local filesystem storage backend.
pub struct LocalFileProvider {
    base: PathBuf,
}

impl LocalFileProvider {
    pub fn new(base: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&base)
            .with_context(|| format!("create VFS base dir {:?}", base))?;
        Ok(Self { base })
    }
}

impl FileProvider for LocalFileProvider {
    async fn store(&self, content: &[u8]) -> anyhow::Result<String> {
        let key = Uuid::now_v7().as_simple().to_string();
        tokio::fs::write(self.base.join(&key), content)
            .await
            .with_context(|| format!("write object {key}"))?;
        Ok(key)
    }

    async fn load(&self, location: &str) -> anyhow::Result<Vec<u8>> {
        tokio::fs::read(self.base.join(location))
            .await
            .with_context(|| format!("read object {location}"))
    }

    async fn delete(&self, location: &str) -> anyhow::Result<()> {
        tokio::fs::remove_file(self.base.join(location))
            .await
            .with_context(|| format!("delete object {location}"))
    }
}
