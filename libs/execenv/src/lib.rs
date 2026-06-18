use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use friday_agent::{AgentConfig, Tool, ToolDesc};
use llm::{Function, Tool as LlmTool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[cfg(feature = "eryx")]
pub use eryx::VolumeMount;

// Pure-Python wheels (py3-none-any). They carry no native extensions, so they
// import on any CPython-WASI minor version and need no compilation.
const BS4_URL: &str = "https://files.pythonhosted.org/packages/88/c6/92fcd42f1ba33e1184263f25bfabf3d27c383410470f169e4b8163bf9c17/beautifulsoup4-4.15.0-py3-none-any.whl";
const SOUPSIEVE_URL: &str = "https://files.pythonhosted.org/packages/5e/f5/0c41cb68dcae6b7de4fac4188a3a9589e21fb31df21ea3a2e888db95e6c9/soupsieve-2.8.4-py3-none-any.whl";
const REQUESTS_URL: &str = "https://files.pythonhosted.org/packages/a0/f4/c67b0b3f1b9245e8d266f0f112c500d50e5b4e83cb6f3b71b6528104182a/requests-2.34.2-py3-none-any.whl";
const URLLIB3_URL: &str = "https://files.pythonhosted.org/packages/7f/3e/5db95bcf282c52709639744ca2a8b149baccf648e39c8cc87553df9eae0c/urllib3-2.7.0-py3-none-any.whl";
const CERTIFI_URL: &str = "https://files.pythonhosted.org/packages/ef/2f/c5464532e965badff2f4c4c1a3a83f5697f0d7c407ed0cda44aaa99bb451/certifi-2026.6.17-py3-none-any.whl";
const IDNA_URL: &str = "https://files.pythonhosted.org/packages/1e/5e/d4e9f1a599fb8e573b7b87160658329fbf28d19eac2718f51fc3def3aa5a/idna-3.18-py3-none-any.whl";
const MARKDOWN_URL: &str = "https://files.pythonhosted.org/packages/de/1f/77fa3081e4f66ca3576c896ae5d31c3002ac6607f9747d2e3aa49227e464/markdown-3.10.2-py3-none-any.whl";
const DATEUTIL_URL: &str = "https://files.pythonhosted.org/packages/36/7a/87837f39d0296e723bb9b62bbb257d0355c7f6128853c78955f57342a56d/python_dateutil-2.8.2-py2.py3-none-any.whl";
const SIX_URL: &str = "https://files.pythonhosted.org/packages/b7/ce/149a00dd41f10bc29e5921b496af8b574d8413afcd5e30dfa0ed46c2cc5e/six-1.17.0-py2.py3-none-any.whl";

#[derive(Clone, Copy)]
enum ArchiveKind {
    /// tar.gz whose root unpacks directly into site-packages. Reserved for
    /// native packages, which are currently deferred (see WASI_PACKAGES).
    #[allow(dead_code)]
    TarGz,
    /// PEP 427 wheel (a zip) whose entries unpack into site-packages.
    Wheel,
}

struct WasiPackage {
    /// Stable key used for the per-package install marker.
    name: &'static str,
    url: &'static str,
    kind: ArchiveKind,
    /// Module to bake into the pre-initialized snapshot. Only native packages
    /// benefit; pure-Python ones load lazily from site-packages at runtime.
    #[cfg_attr(not(feature = "eryx"), allow(dead_code))]
    preinit_import: Option<&'static str>,
}

// Native packages (numpy, Pillow, pandas, ...) are intentionally absent. The
// prebuilt WASI wheels from bkmashiro/wasi-wheels are CPython 3.14 / wasm32-wasip1
// but were built against a different wasi-libc than eryx-runtime 0.4.9 (which
// links with wasi-sdk-27): their `.so` files need an unresolved `__wasi_init_tp`
// symbol and fail preinit linking, which would break the whole runtime. A
// working native package must be compiled against eryx-runtime's exact toolchain
// (wasi-sdk-27 + its CPython 3.14). Add such packages here as ArchiveKind::TarGz.
const WASI_PACKAGES: &[WasiPackage] = &[
    WasiPackage {
        name: "beautifulsoup4",
        url: BS4_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "soupsieve",
        url: SOUPSIEVE_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "requests",
        url: REQUESTS_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "urllib3",
        url: URLLIB3_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "certifi",
        url: CERTIFI_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "idna",
        url: IDNA_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "markdown",
        url: MARKDOWN_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "python-dateutil",
        url: DATEUTIL_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
    WasiPackage {
        name: "six",
        url: SIX_URL,
        kind: ArchiveKind::Wheel,
        preinit_import: None,
    },
];

#[cfg(feature = "eryx")]
fn preinit_imports() -> Vec<&'static str> {
    WASI_PACKAGES
        .iter()
        .filter_map(|pkg| pkg.preinit_import)
        .collect()
}

#[cfg(feature = "eryx")]
const ERYX_RUNTIME_CACHE_VERSION: &str = "2";

#[derive(Clone, Debug)]
pub enum BackendKind {
    Mock,
    Eryx,
}

#[derive(Clone, Debug)]
pub enum NetworkAccess {
    Blocked,
    Allowed,
}

#[derive(Clone, Debug)]
pub struct ExecutionLimits {
    pub max_runtime: Duration,
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_fuel: Option<u64>,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_runtime: Duration::from_secs(30),
            max_memory_bytes: Some(128 * 1024 * 1024),
            max_cpu_fuel: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PythonToolConfig {
    pub cache_dir: PathBuf,
    pub backend: BackendKind,
    pub threads: usize,
    pub preinit: bool,
    pub limits: ExecutionLimits,
    pub network: NetworkAccess,
}

impl Default for PythonToolConfig {
    fn default() -> Self {
        Self {
            cache_dir: std::env::temp_dir().join("friday-execenv"),
            backend: BackendKind::Mock,
            threads: 1,
            preinit: true,
            limits: ExecutionLimits::default(),
            network: NetworkAccess::Blocked,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait ExecutionService: Send + Sync {
    async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput>;
}

#[async_trait]
pub trait FileSystemBackend: Send + Sync {
    async fn before_execute(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn after_execute(&self) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "eryx")]
    fn volumes(&self) -> Vec<VolumeMount> {
        Vec::new()
    }
}

pub struct DirectOsFileSystem {
    #[cfg_attr(not(feature = "eryx"), allow(dead_code))]
    host_dir: PathBuf,
    guest_dir: String,
    read_only: bool,
}

impl DirectOsFileSystem {
    pub fn new(host_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&host_dir)?;
        Ok(Self {
            host_dir,
            guest_dir: "/workspace".to_string(),
            read_only: false,
        })
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn guest_dir(mut self, guest_dir: impl Into<String>) -> Self {
        self.guest_dir = guest_dir.into();
        self
    }
}

#[async_trait]
impl FileSystemBackend for DirectOsFileSystem {
    #[cfg(feature = "eryx")]
    fn volumes(&self) -> Vec<VolumeMount> {
        let mount = if self.read_only {
            VolumeMount::read_only(&self.host_dir, &self.guest_dir)
        } else {
            VolumeMount::new(&self.host_dir, &self.guest_dir)
        };
        vec![mount]
    }
}

#[derive(Default)]
pub struct MockExecutionService;

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute_python(&self, _script: &str) -> anyhow::Result<ExecutionOutput> {
        Ok(ExecutionOutput {
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

pub struct PythonTool {
    service: Arc<dyn ExecutionService>,
}

impl PythonTool {
    pub async fn new(
        config: PythonToolConfig,
        fs: Arc<dyn FileSystemBackend>,
    ) -> anyhow::Result<Self> {
        let service: Arc<dyn ExecutionService> = match config.backend {
            BackendKind::Mock => Arc::new(MockExecutionService),
            BackendKind::Eryx => make_eryx_service(config, fs).await?,
        };
        Ok(Self { service })
    }

    pub fn from_service(service: Arc<dyn ExecutionService>) -> Self {
        Self { service }
    }
}

#[derive(ToolDesc)]
struct PythonParams {
    /// Python script to execute. With the Eryx backend these packages are
    /// available: requests, beautifulsoup4 (bs4), urllib3, certifi, idna,
    /// markdown, python-dateutil (dateutil), six. Network access is required
    /// for requests to reach remote hosts.
    script: String,
}

#[async_trait(?Send)]
impl Tool for PythonTool {
    fn name(&self) -> &str {
        "python"
    }

    fn readable_name(&self) -> &str {
        "Python"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Execute a Python script in a sandbox and return stdout and stderr. \
                    Available packages: requests, beautifulsoup4, urllib3, certifi, idna, \
                    markdown, python-dateutil, six."
                    .to_string(),
                name: self.name().to_string(),
                parameters: Some(PythonParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match PythonParams::decode(args) {
            Ok(params) => params,
            Err(err) => return json!({ "error": err }),
        };

        match self.service.execute_python(&params.script).await {
            Ok(output) => json!({
                "success": true,
                "stdout": output.stdout,
                "stderr": output.stderr,
            }),
            Err(err) => json!({
                "success": false,
                "stdout": "",
                "stderr": "",
                "error": format!("{err:#}"),
            }),
        }
    }
}

pub async fn ensure_wasi_dependencies(cache_dir: &Path) -> anyhow::Result<WasiDependencies> {
    tokio::fs::create_dir_all(cache_dir).await?;
    let deps_dir = cache_dir.join("deps");
    let site_packages = deps_dir.join("site-packages");
    let markers = deps_dir.join(".installed");
    tokio::fs::create_dir_all(&site_packages).await?;
    tokio::fs::create_dir_all(&markers).await?;

    for pkg in WASI_PACKAGES {
        install_package(pkg, &deps_dir, &site_packages, &markers).await?;
    }

    Ok(WasiDependencies { site_packages })
}

async fn install_package(
    pkg: &WasiPackage,
    deps_dir: &Path,
    site_packages: &Path,
    markers: &Path,
) -> anyhow::Result<()> {
    let marker = markers.join(pkg.name);
    let installed = tokio::fs::read_to_string(&marker).await.ok();
    if installed.as_deref() == Some(pkg.url) {
        return Ok(());
    }

    // First install or the source URL changed: fetch fresh. The cached archive
    // is removed first because different sources can share a filename (both
    // dicej and bkmashiro publish `numpy-wasi.tar.gz`).
    let archive = deps_dir.join(archive_file_name(pkg));
    let _ = tokio::fs::remove_file(&archive).await;
    download(pkg.url, &archive).await?;
    match pkg.kind {
        ArchiveKind::TarGz => extract_tar_gz(&archive, site_packages).await?,
        ArchiveKind::Wheel => extract_zip(&archive, site_packages).await?,
    }
    tokio::fs::write(&marker, pkg.url).await?;
    Ok(())
}

fn archive_file_name(pkg: &WasiPackage) -> &'static str {
    pkg.url.rsplit('/').next().unwrap_or(pkg.name)
}

#[derive(Clone, Debug)]
pub struct WasiDependencies {
    pub site_packages: PathBuf,
}

async fn download(url: &str, path: &Path) -> anyhow::Result<()> {
    let bytes = tokio::time::timeout(Duration::from_secs(60), fetch(url)).await??;
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

const MAX_REDIRECTS: usize = 10;

async fn fetch(url: &str) -> anyhow::Result<bytes::Bytes> {
    use http_body_util::{BodyExt, Empty};
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
        let req = hyper::Request::builder()
            .uri(&url)
            .body(Empty::<bytes::Bytes>::new())?;
        let res = client.request(req).await?;
        let status = res.status();

        if status.is_redirection() {
            let location = res
                .headers()
                .get(LOCATION)
                .ok_or_else(|| anyhow::anyhow!("redirect {status} without location header"))?
                .to_str()?;
            url = resolve_redirect(&url, location)?;
            continue;
        }

        anyhow::ensure!(status.is_success(), "download failed with status {status}");
        return Ok(res.into_body().collect().await?.to_bytes());
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
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(target)?;
        Ok(())
    })
    .await?
}

async fn extract_zip(archive: &Path, target: &Path) -> anyhow::Result<()> {
    let archive = archive.to_path_buf();
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        zip.extract(target)?;
        Ok(())
    })
    .await?
}

#[cfg(feature = "eryx")]
mod eryx_backend {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex, OnceLock, mpsc},
    };

    use anyhow::Context;
    use async_trait::async_trait;
    use tokio::sync::{OnceCell, oneshot};

    use crate::{
        ERYX_RUNTIME_CACHE_VERSION, ExecutionLimits, ExecutionOutput, ExecutionService,
        FileSystemBackend, NetworkAccess, PythonToolConfig, ensure_wasi_dependencies,
    };

    pub struct EryxExecutionService {
        config: PythonToolConfig,
        fs: Arc<dyn FileSystemBackend>,
        runtime: Arc<OnceCell<PreinitRuntime>>,
        hub: Arc<ExecutionHub>,
    }

    impl EryxExecutionService {
        pub async fn new(
            config: PythonToolConfig,
            fs: Arc<dyn FileSystemBackend>,
        ) -> anyhow::Result<Self> {
            let runtime = runtime_cell(&config);
            let hub = execution_hub(&config);
            Ok(Self {
                config,
                fs,
                runtime,
                hub,
            })
        }
    }

    pub(super) async fn prepare_runtime(config: PythonToolConfig) -> anyhow::Result<()> {
        let runtime = runtime_cell(&config);
        let hub = execution_hub(&config);
        runtime
            .get_or_try_init(|| hub.prepare(config.clone()))
            .await
            .map(|_| ())
    }

    #[async_trait]
    impl ExecutionService for EryxExecutionService {
        async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput> {
            let runtime = self
                .runtime
                .get_or_try_init(|| self.hub.prepare(self.config.clone()))
                .await?;
            self.fs.before_execute().await?;
            let result = self
                .hub
                .execute(ExecutionRequest {
                    runtime: Arc::new(runtime.clone()),
                    script: script.to_string(),
                    limits: self.config.limits.clone(),
                    volumes: self.fs.volumes(),
                    network: self.config.network.clone(),
                })
                .await;
            let after = self.fs.after_execute().await;

            match (result, after) {
                (Ok(output), Ok(())) => Ok(output),
                (Err(err), _) => Err(err),
                (Ok(_), Err(err)) => Err(err).context("sync eryx filesystem after execute"),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct RuntimeKey {
        cache_dir: PathBuf,
        preinit: bool,
    }

    fn runtime_cell(config: &PythonToolConfig) -> Arc<OnceCell<PreinitRuntime>> {
        let key = RuntimeKey {
            cache_dir: config.cache_dir.clone(),
            preinit: config.preinit,
        };
        static RUNTIMES: OnceLock<Mutex<HashMap<RuntimeKey, Arc<OnceCell<PreinitRuntime>>>>> =
            OnceLock::new();
        let runtimes = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut runtimes = runtimes.lock().expect("execenv runtime registry poisoned");
        runtimes
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct HubKey {
        cache_dir: PathBuf,
        preinit: bool,
        threads: usize,
    }

    fn execution_hub(config: &PythonToolConfig) -> Arc<ExecutionHub> {
        let threads = config.threads.max(1);
        let key = HubKey {
            cache_dir: config.cache_dir.clone(),
            preinit: config.preinit,
            threads,
        };
        static HUBS: OnceLock<Mutex<HashMap<HubKey, Arc<ExecutionHub>>>> = OnceLock::new();
        let hubs = HUBS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut hubs = hubs.lock().expect("execenv hub registry poisoned");
        hubs.entry(key)
            .or_insert_with(|| Arc::new(ExecutionHub::new(threads)))
            .clone()
    }

    async fn build_runtime(config: PythonToolConfig) -> anyhow::Result<PreinitRuntime> {
        if config.preinit {
            let deps = ensure_wasi_dependencies(&config.cache_dir).await?;
            let imports = crate::preinit_imports();
            prepare_preinit(&config.cache_dir, Some(&deps.site_packages), &imports).await
        } else {
            prepare_preinit(&config.cache_dir, None, &[]).await
        }
    }

    #[derive(Clone)]
    struct PreinitRuntime {
        runtime: PathBuf,
        stdlib: PathBuf,
        site_packages: Option<PathBuf>,
        executor: Arc<eryx::PythonExecutor>,
    }

    async fn prepare_preinit(
        cache_dir: &std::path::Path,
        site_packages: Option<&std::path::Path>,
        imports: &[&str],
    ) -> anyhow::Result<PreinitRuntime> {
        let preinit_dir = cache_dir.join("preinit");
        tokio::fs::create_dir_all(&preinit_dir).await?;
        let runtime = preinit_dir.join(if imports.is_empty() {
            "python.cwasm"
        } else {
            "python-numpy.cwasm"
        });
        let marker = runtime.with_extension("version");
        let stdlib = eryx::embedded_stdlib::EmbeddedStdlib::get()?
            .path()
            .to_path_buf();

        if runtime.exists() && cache_marker_matches(&marker).await? {
            return preinit_runtime(
                runtime,
                stdlib,
                site_packages.map(std::path::Path::to_path_buf),
            );
        }

        let extensions = if let Some(site_packages) = site_packages {
            let package = eryx::ExtractedPackage::from_path(site_packages)?;
            package
                .native_extensions
                .iter()
                .map(|ext| {
                    eryx::preinit::NativeExtension::new(
                        format!("/site-packages/{}", ext.relative_path),
                        ext.bytes.clone(),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        let component =
            eryx::preinit::pre_initialize(&stdlib, site_packages, imports, &extensions).await?;
        let precompiled = eryx::PythonExecutor::precompile(&component)?;
        let tmp = runtime.with_extension("cwasm.tmp");
        tokio::fs::write(&tmp, precompiled).await?;
        tokio::fs::rename(tmp, &runtime).await?;
        tokio::fs::write(&marker, ERYX_RUNTIME_CACHE_VERSION).await?;

        preinit_runtime(
            runtime,
            stdlib,
            site_packages.map(std::path::Path::to_path_buf),
        )
    }

    async fn cache_marker_matches(path: &std::path::Path) -> anyhow::Result<bool> {
        match tokio::fs::read_to_string(path).await {
            Ok(version) => Ok(version.trim() == ERYX_RUNTIME_CACHE_VERSION),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).context("read eryx runtime cache marker"),
        }
    }

    fn preinit_runtime(
        runtime: PathBuf,
        stdlib: PathBuf,
        site_packages: Option<PathBuf>,
    ) -> anyhow::Result<PreinitRuntime> {
        let mut executor = unsafe {
            eryx::PythonExecutor::from_precompiled_file(&runtime)
                .context("load precompiled eryx runtime")?
        }
        .with_python_stdlib(&stdlib);
        if let Some(site_packages) = site_packages.as_ref() {
            executor = executor.with_site_packages(site_packages);
        }

        Ok(PreinitRuntime {
            runtime,
            stdlib,
            site_packages,
            executor: Arc::new(executor),
        })
    }

    struct ExecutionHub {
        tx: mpsc::Sender<ExecutionJob>,
    }

    impl ExecutionHub {
        fn new(threads: usize) -> Self {
            let (tx, rx) = mpsc::channel();
            let rx = Arc::new(Mutex::new(rx));
            for idx in 0..threads {
                let rx = rx.clone();
                std::thread::Builder::new()
                    .name(format!("friday-eryx-{idx}"))
                    .spawn(move || worker_loop(rx))
                    .expect("eryx worker thread");
            }
            Self { tx }
        }

        async fn execute(&self, request: ExecutionRequest) -> anyhow::Result<ExecutionOutput> {
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(ExecutionJob::Execute { request, tx })
                .map_err(|_| anyhow::anyhow!("eryx execution queue stopped"))?;
            rx.await.context("eryx worker stopped")?
        }

        async fn prepare(&self, config: PythonToolConfig) -> anyhow::Result<PreinitRuntime> {
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(ExecutionJob::Prepare { config, tx })
                .map_err(|_| anyhow::anyhow!("eryx execution queue stopped"))?;
            rx.await.context("eryx worker stopped")?
        }
    }

    enum ExecutionJob {
        Prepare {
            config: PythonToolConfig,
            tx: oneshot::Sender<anyhow::Result<PreinitRuntime>>,
        },
        Execute {
            request: ExecutionRequest,
            tx: oneshot::Sender<anyhow::Result<ExecutionOutput>>,
        },
    }

    struct ExecutionRequest {
        runtime: Arc<PreinitRuntime>,
        script: String,
        limits: ExecutionLimits,
        volumes: Vec<eryx::VolumeMount>,
        network: NetworkAccess,
    }

    fn worker_loop(rx: Arc<Mutex<mpsc::Receiver<ExecutionJob>>>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("eryx worker runtime");

        loop {
            let job = {
                let rx = rx.lock().expect("eryx execution queue poisoned");
                rx.recv()
            };
            let Ok(job) = job else {
                break;
            };
            match job {
                ExecutionJob::Prepare { config, tx } => {
                    let result = runtime.block_on(build_runtime(config));
                    let _ = tx.send(result);
                }
                ExecutionJob::Execute { request, tx } => {
                    let result = runtime.block_on(execute_request(request));
                    let _ = tx.send(result);
                }
            }
        }
    }

    async fn execute_request(request: ExecutionRequest) -> anyhow::Result<ExecutionOutput> {
        if matches!(request.network, NetworkAccess::Allowed) {
            return execute_request_with_network(request).await;
        }

        let mut execute = request
            .runtime
            .executor
            .execute(request.script)
            .with_timeout(request.limits.max_runtime);
        if let Some(limit) = request.limits.max_memory_bytes {
            execute = execute.with_memory_limit(limit);
        }
        if let Some(fuel) = request.limits.max_cpu_fuel {
            execute = execute.with_fuel_limit(fuel);
        }
        if !request.volumes.is_empty() {
            execute = execute.with_volumes(request.volumes);
        }

        let result = execute.run().await.context("execute eryx script")?;
        Ok(ExecutionOutput {
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }

    async fn execute_request_with_network(
        request: ExecutionRequest,
    ) -> anyhow::Result<ExecutionOutput> {
        let mut builder = unsafe {
            eryx::Sandbox::builder()
                .with_precompiled_file(&request.runtime.runtime)
                .with_python_stdlib(&request.runtime.stdlib)
        }
        .with_resource_limits(to_eryx_limits(&request.limits))
        .with_volumes(request.volumes)
        .with_network(eryx::NetConfig::permissive());
        if let Some(site_packages) = request.runtime.site_packages.as_ref() {
            builder = builder.with_site_packages(site_packages);
        }
        let sandbox = builder.build().context("build eryx sandbox")?;
        let result = sandbox
            .execute(&request.script)
            .await
            .context("execute eryx script")?;
        Ok(ExecutionOutput {
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }

    fn to_eryx_limits(limits: &ExecutionLimits) -> eryx::ResourceLimits {
        eryx::ResourceLimits {
            execution_timeout: Some(limits.max_runtime),
            max_memory_bytes: limits.max_memory_bytes,
            max_fuel: limits.max_cpu_fuel,
            ..Default::default()
        }
    }
}

#[cfg(feature = "eryx")]
pub async fn prepare_eryx_runtime(config: PythonToolConfig) -> anyhow::Result<()> {
    eryx_backend::prepare_runtime(config).await
}

#[cfg(not(feature = "eryx"))]
pub async fn prepare_eryx_runtime(_config: PythonToolConfig) -> anyhow::Result<()> {
    anyhow::bail!("execenv was built without the eryx feature")
}

#[cfg(feature = "eryx")]
async fn make_eryx_service(
    config: PythonToolConfig,
    fs: Arc<dyn FileSystemBackend>,
) -> anyhow::Result<Arc<dyn ExecutionService>> {
    Ok(Arc::new(
        eryx_backend::EryxExecutionService::new(config, fs).await?,
    ))
}

#[cfg(not(feature = "eryx"))]
async fn make_eryx_service(
    _config: PythonToolConfig,
    _fs: Arc<dyn FileSystemBackend>,
) -> anyhow::Result<Arc<dyn ExecutionService>> {
    anyhow::bail!("execenv was built without the eryx feature")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "network: downloads a tarball through a GitHub release redirect"]
    async fn download_follows_redirects_and_fetches_tarball() {
        // A GitHub release asset that 302-redirects to release-assets storage,
        // exercising the redirect-following path of `download`.
        let url =
            "https://github.com/bkmashiro/wasi-wheels/releases/download/latest/numpy-wasi.tar.gz";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("numpy-wasi.tar.gz");
        download(url, &path).await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert!(bytes.len() > 1024, "got {} bytes", bytes.len());
        assert_eq!(&bytes[..2], &[0x1f, 0x8b], "not a gzip stream");
    }

    #[test]
    fn archive_file_name_uses_last_url_segment() {
        let pkg = WasiPackage {
            name: "demo",
            url: "https://example.com/path/demo-1.0-py3-none-any.whl",
            kind: ArchiveKind::Wheel,
            preinit_import: None,
        };
        assert_eq!(archive_file_name(&pkg), "demo-1.0-py3-none-any.whl");
    }

    #[test]
    fn package_manifest_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        for pkg in WASI_PACKAGES {
            assert!(
                names.insert(pkg.name),
                "duplicate package name {}",
                pkg.name
            );
            assert!(
                pkg.url.starts_with("https://"),
                "{} url not https",
                pkg.name
            );
            assert!(
                !archive_file_name(pkg).is_empty(),
                "{} has empty archive name",
                pkg.name
            );
        }
    }

    #[tokio::test]
    async fn mock_service_returns_empty_output() {
        let output = MockExecutionService
            .execute_python("print('ignored')")
            .await
            .unwrap();

        assert_eq!(
            output,
            ExecutionOutput {
                stdout: String::new(),
                stderr: String::new()
            }
        );
    }

    #[tokio::test]
    async fn python_tool_wraps_service_output() {
        struct EchoService;

        #[async_trait]
        impl ExecutionService for EchoService {
            async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput> {
                Ok(ExecutionOutput {
                    stdout: script.to_string(),
                    stderr: String::new(),
                })
            }
        }

        let tool = PythonTool::from_service(Arc::new(EchoService));
        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: friday_agent::ModelRegistry::new(),
                    max_iterations: 1,
                }),
                json!({ "script": "print(1)" }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"], "print(1)");
    }

    #[cfg(feature = "eryx")]
    #[tokio::test]
    #[ignore = "precompiles Eryx runtime"]
    async fn eryx_backend_executes_base_python_without_numpy() {
        let workspace = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let fs = Arc::new(
            DirectOsFileSystem::new(workspace.path().join("workspace"))
                .unwrap()
                .guest_dir("/workspace"),
        );
        let config = PythonToolConfig {
            cache_dir: cache.path().to_path_buf(),
            backend: BackendKind::Eryx,
            threads: 2,
            preinit: false,
            limits: ExecutionLimits {
                max_runtime: Duration::from_secs(5),
                max_memory_bytes: Some(128 * 1024 * 1024),
                max_cpu_fuel: None,
            },
            network: NetworkAccess::Blocked,
        };
        let tool = PythonTool::new(config, fs).await.unwrap();

        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: friday_agent::ModelRegistry::new(),
                    max_iterations: 1,
                }),
                json!({ "script": "print(2 + 2)" }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "4");
    }

    #[cfg(feature = "eryx")]
    #[tokio::test]
    #[ignore = "downloads pure-Python wheels and precompiles runtime"]
    async fn eryx_backend_imports_pure_python_packages() {
        let workspace = tempfile::tempdir().unwrap();
        let cache_dir = std::env::temp_dir().join("friday-execenv-pure-test-cache");
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();
        let fs = Arc::new(
            DirectOsFileSystem::new(workspace.path().join("workspace"))
                .unwrap()
                .guest_dir("/workspace"),
        );
        let config = PythonToolConfig {
            cache_dir,
            backend: BackendKind::Eryx,
            threads: 1,
            preinit: true,
            limits: ExecutionLimits::default(),
            network: NetworkAccess::Blocked,
        };
        prepare_eryx_runtime(config.clone()).await.unwrap();
        let tool = PythonTool::new(config, fs).await.unwrap();

        // No network needed: exercise imports and offline behaviour only.
        let script = "import markdown, dateutil, requests\n\
             from bs4 import BeautifulSoup\n\
             html = markdown.markdown('# Title')\n\
             text = BeautifulSoup(html, 'html.parser').get_text().strip()\n\
             print(text)";
        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: friday_agent::ModelRegistry::new(),
                    max_iterations: 1,
                }),
                json!({ "script": script }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "Title");
    }
}
