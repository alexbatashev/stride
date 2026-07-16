use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug)]
pub enum ArtifactKind {
    /// A gzip-compressed tar archive.
    TarGz,
    /// A ZIP archive, including Python wheels.
    Zip,
    /// A single file retained under its URL filename.
    File,
}

#[derive(Clone, Copy, Debug)]
pub struct ArtifactSpec {
    /// Stable cache and marker key.
    pub name: &'static str,
    /// Download source.
    pub url: &'static str,
    /// Lowercase hexadecimal SHA-256 digest.
    pub sha256: &'static str,
    /// Downloaded content format.
    pub kind: ArtifactKind,
}

#[derive(Clone, Debug)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    /// Creates a store rooted at an existing or new cache directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Downloads, verifies, and extracts an artifact when its marker is stale.
    pub async fn ensure(&self, spec: &ArtifactSpec) -> anyhow::Result<PathBuf> {
        let target = self.root.join(spec.name);
        self.ensure_at(spec, &target, false).await?;
        Ok(target)
    }

    /// Keeps an existing artifact available when refreshing it fails.
    pub async fn ensure_best_effort(&self, spec: &ArtifactSpec) -> Option<PathBuf> {
        let target = self.root.join(spec.name);
        match self.ensure_at(spec, &target, false).await {
            Ok(()) => Some(target),
            Err(err) => {
                tracing::warn!(artifact = spec.name, %err, "failed to provision artifact");
                target.exists().then_some(target)
            }
        }
    }

    pub(crate) async fn ensure_into(
        &self,
        spec: &ArtifactSpec,
        target: &Path,
    ) -> anyhow::Result<PathBuf> {
        self.ensure_at(spec, target, true).await?;
        Ok(target.to_path_buf())
    }

    async fn ensure_at(
        &self,
        spec: &ArtifactSpec,
        target: &Path,
        overlay: bool,
    ) -> anyhow::Result<()> {
        validate_spec(spec)?;
        let marker = self.root.join(".installed").join(spec.name);
        let expected_marker = marker_contents(spec);
        if tokio::fs::read_to_string(&marker).await.ok().as_deref()
            == Some(expected_marker.as_str())
            && target.exists()
        {
            return Ok(());
        }

        tokio::fs::create_dir_all(&self.root).await?;
        let downloads = self.root.join(".downloads");
        tokio::fs::create_dir_all(&downloads).await?;
        let archive = downloads.join(spec.name);
        let _ = tokio::fs::remove_file(&archive).await;

        download(spec.url, &archive).await?;
        if let Err(err) = verify_sha256(&archive, spec.sha256).await {
            tracing::warn!(artifact = spec.name, %err, "artifact hash mismatch");
            let _ = tokio::fs::remove_file(&archive).await;
            return Err(err);
        }

        let staging = self.root.join(".extracting").join(spec.name);
        let extraction_target = if overlay {
            target
        } else {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            tokio::fs::create_dir_all(&staging).await?;
            &staging
        };
        tokio::fs::create_dir_all(extraction_target).await?;
        match spec.kind {
            ArtifactKind::TarGz => extract_tar_gz(&archive, extraction_target).await?,
            ArtifactKind::Zip => extract_zip(&archive, extraction_target).await?,
            ArtifactKind::File => {
                let file_name = spec.url.rsplit('/').next().unwrap_or(spec.name);
                tokio::fs::copy(&archive, extraction_target.join(file_name)).await?;
            }
        }
        if !overlay {
            let _ = tokio::fs::remove_dir_all(target).await;
            tokio::fs::rename(&staging, target).await?;
        }

        tokio::fs::create_dir_all(marker.parent().expect("marker has parent")).await?;
        tokio::fs::write(marker, expected_marker).await?;
        let _ = tokio::fs::remove_file(archive).await;
        Ok(())
    }
}

fn validate_spec(spec: &ArtifactSpec) -> anyhow::Result<()> {
    anyhow::ensure!(!spec.name.is_empty(), "artifact name is empty");
    anyhow::ensure!(
        spec.name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
        "invalid artifact name: {}",
        spec.name
    );
    anyhow::ensure!(
        spec.sha256.len() == 64 && spec.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid sha256 for {}",
        spec.name
    );
    Ok(())
}

fn marker_contents(spec: &ArtifactSpec) -> String {
    format!("{}\n{}", spec.url, spec.sha256.to_ascii_lowercase())
}

async fn verify_sha256(path: &Path, expected: &str) -> anyhow::Result<()> {
    let bytes = tokio::fs::read(path).await?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    anyhow::ensure!(
        actual == expected.to_ascii_lowercase(),
        "sha256 mismatch: expected {expected}, got {actual}"
    );
    Ok(())
}

pub(crate) async fn download(url: &str, path: &Path) -> anyhow::Result<()> {
    let bytes = tokio::time::timeout(Duration::from_secs(60), fetch(url))
        .await
        .context("download timed out")??;
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

const MAX_REDIRECTS: usize = 10;

pub(crate) async fn fetch(url: &str) -> anyhow::Result<bytes::Bytes> {
    use http_body_util::{BodyExt as _, Empty};
    use hyper::header::LOCATION;

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_or_http()
        .enable_http1()
        .build();
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build::<_, Empty<bytes::Bytes>>(https);

    let mut url = url.to_string();
    for _ in 0..MAX_REDIRECTS {
        let request = hyper::Request::builder()
            .uri(&url)
            .header(hyper::header::USER_AGENT, "stride-execenv/0.1")
            .body(Empty::<bytes::Bytes>::new())?;
        let response = client.request(request).await?;
        let status = response.status();

        if status.is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .ok_or_else(|| anyhow::anyhow!("redirect {status} without location header"))?
                .to_str()?;
            url = resolve_redirect(&url, location)?;
            continue;
        }

        anyhow::ensure!(status.is_success(), "download failed with status {status}");
        return Ok(response.into_body().collect().await?.to_bytes());
    }

    anyhow::bail!("too many redirects")
}

fn resolve_redirect(base: &str, location: &str) -> anyhow::Result<String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_string());
    }
    let base: hyper::Uri = base.parse()?;
    let scheme = base.scheme_str().unwrap_or("https");
    let authority = base
        .authority()
        .ok_or_else(|| anyhow::anyhow!("base url missing authority"))?;
    let sep = if location.starts_with('/') { "" } else { "/" };
    Ok(format!("{scheme}://{authority}{sep}{location}"))
}

async fn extract_tar_gz(archive: &Path, target: &Path) -> anyhow::Result<()> {
    let archive = archive.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let file = std::fs::File::open(archive)?;
        let decoder = flate2::read::GzDecoder::new(file);
        tar::Archive::new(decoder).unpack(target)?;
        Ok(())
    })
    .await?
}

async fn extract_zip(archive: &Path, target: &Path) -> anyhow::Result<()> {
    let archive = archive.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let file = std::fs::File::open(archive)?;
        zip::ZipArchive::new(file)?.extract(target)?;
        Ok(())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn zip_fixture() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("payload.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"artifact contents").unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn serve(body: Vec<u8>) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                let mut request = [0; 1024];
                let _ = stream.read(&mut request);
                request_count.fetch_add(1, Ordering::SeqCst);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        (format!("http://{address}/artifact"), requests, handle)
    }

    #[tokio::test]
    async fn cold_cache_downloads_and_warm_cache_does_not() {
        let body = zip_fixture();
        let hash = format!("{:x}", Sha256::digest(&body));
        let (url, requests, server) = serve(body);
        let spec = ArtifactSpec {
            name: "fixture",
            url: Box::leak(url.into_boxed_str()),
            sha256: Box::leak(hash.into_boxed_str()),
            kind: ArtifactKind::Zip,
        };
        let cache = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(cache.path());

        let path = store.ensure(&spec).await.unwrap();
        assert_eq!(
            tokio::fs::read(path.join("payload.txt")).await.unwrap(),
            b"artifact contents"
        );
        store.ensure(&spec).await.unwrap();

        server.join().unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            tokio::fs::read_to_string(cache.path().join(".installed/fixture"))
                .await
                .unwrap(),
            marker_contents(&spec)
        );
    }

    #[tokio::test]
    async fn hash_mismatch_is_deleted_and_not_installed() {
        let (url, _, server) = serve(b"tampered".to_vec());
        let spec = ArtifactSpec {
            name: "fixture",
            url: Box::leak(url.into_boxed_str()),
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            kind: ArtifactKind::File,
        };
        let cache = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(cache.path());

        let error = store.ensure(&spec).await.unwrap_err();

        server.join().unwrap();
        assert!(error.to_string().contains("sha256 mismatch"));
        assert!(!cache.path().join(".downloads/fixture").exists());
        assert!(!cache.path().join(".installed/fixture").exists());
    }

    #[tokio::test]
    async fn overlay_artifact_preserves_sibling_packages() {
        let body = zip_fixture();
        let hash = format!("{:x}", Sha256::digest(&body));
        let (url, _, server) = serve(body);
        let spec = ArtifactSpec {
            name: "fixture",
            url: Box::leak(url.into_boxed_str()),
            sha256: Box::leak(hash.into_boxed_str()),
            kind: ArtifactKind::Zip,
        };
        let cache = tempfile::tempdir().unwrap();
        let target = cache.path().join("site-packages");
        tokio::fs::create_dir(&target).await.unwrap();
        tokio::fs::write(target.join("sibling.py"), b"sibling")
            .await
            .unwrap();

        ArtifactStore::new(cache.path())
            .ensure_into(&spec, &target)
            .await
            .unwrap();

        server.join().unwrap();
        assert_eq!(
            tokio::fs::read(target.join("payload.txt")).await.unwrap(),
            b"artifact contents"
        );
        assert_eq!(
            tokio::fs::read(target.join("sibling.py")).await.unwrap(),
            b"sibling"
        );
    }

    #[tokio::test]
    async fn best_effort_keeps_previous_artifact_on_network_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/artifact", listener.local_addr().unwrap());
        drop(listener);
        let spec = ArtifactSpec {
            name: "fixture",
            url: Box::leak(url.into_boxed_str()),
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            kind: ArtifactKind::File,
        };
        let cache = tempfile::tempdir().unwrap();
        let previous = cache.path().join("fixture");
        tokio::fs::create_dir(&previous).await.unwrap();
        tokio::fs::write(previous.join("old"), b"previous")
            .await
            .unwrap();

        let path = ArtifactStore::new(cache.path())
            .ensure_best_effort(&spec)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(path.join("old")).await.unwrap(),
            b"previous"
        );
    }
}
