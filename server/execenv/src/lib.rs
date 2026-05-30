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

const NUMPY_WASI_URL: &str =
    "https://github.com/dicej/wasi-wheels/releases/download/v0.0.2/numpy-wasi.tar.gz";
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
    /// Python script to execute. Numpy is available as numpy/np when the Eryx backend is enabled.
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
                description: "Execute a Python script in a sandbox and return stdout and stderr."
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
    tokio::fs::create_dir_all(&deps_dir).await?;

    let numpy_tarball = deps_dir.join("numpy-wasi.tar.gz");
    if !numpy_tarball.exists() {
        download(NUMPY_WASI_URL, &numpy_tarball).await?;
    }

    let site_packages = deps_dir.join("site-packages");
    if !site_packages.join("numpy").is_dir() {
        let tmp = deps_dir.join("site-packages.tmp");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        tokio::fs::create_dir_all(&tmp).await?;
        extract_tar_gz(&numpy_tarball, &tmp).await?;
        if site_packages.exists() {
            tokio::fs::remove_dir_all(&site_packages).await?;
        }
        tokio::fs::rename(&tmp, &site_packages).await?;
    }

    Ok(WasiDependencies {
        numpy_tarball,
        site_packages,
    })
}

#[derive(Clone, Debug)]
pub struct WasiDependencies {
    pub numpy_tarball: PathBuf,
    pub site_packages: PathBuf,
}

async fn download(url: &str, path: &Path) -> anyhow::Result<()> {
    let url = url.to_string();
    let bytes = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .build()
            .into();
        let mut response = agent.get(&url).call()?;
        Ok(response.body_mut().read_to_vec()?)
    })
    .await??;
    tokio::fs::write(path, bytes).await?;
    Ok(())
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
            prepare_preinit(&config.cache_dir, Some(&deps.site_packages), &["numpy"]).await
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
    #[ignore = "downloads numpy WASI package"]
    async fn eryx_backend_executes_numpy() {
        let workspace = tempfile::tempdir().unwrap();
        let cache_dir = std::env::temp_dir().join("friday-execenv-test-cache");
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

        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: friday_agent::ModelRegistry::new(),
                    max_iterations: 1,
                }),
                json!({ "script": "import numpy as np\nprint(np.array([1, 2, 3]).sum())" }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "6");
    }
}
