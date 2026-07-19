use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Bytes;
use sha2::{Digest as _, Sha256};
use wasmtime::component::{Component, Linker as ComponentLinker, ResourceTable};
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Strategy};
use wasmtime_wasi::p1;
use wasmtime_wasi::p2::bindings::sync::Command as P2Command;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::execution_hub::{ExecutionHub, execution_hub};
use crate::{
    ArtifactKind, ArtifactSpec, ArtifactStore, ExecutionLimits, ExecutionWorkspace, NetworkAccess,
    VolumeMount,
};

const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const EPOCH_TICK: Duration = Duration::from_millis(10);
const COMMAND_CACHE_VERSION: &str = "2";

#[derive(Clone, Debug)]
pub struct ExecInvocation {
    pub argv: Vec<String>,
    pub stdin: Vec<u8>,
    pub cwd: String,
    pub timeout: Option<Duration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub returncode: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

#[derive(Clone)]
pub struct PreparedCommand {
    kind: PreparedCommandKind,
}

#[derive(Clone)]
enum PreparedCommandKind {
    P1(Module),
    P2(Component),
}

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub name: &'static str,
    pub artifact: ArtifactSpec,
    pub description: &'static str,
    pub network: NetworkAccess,
    pub limits: ExecutionLimits,
}

pub const PANDOC: CommandSpec = CommandSpec {
    name: "pandoc",
    artifact: ArtifactSpec {
        name: "pandoc-wasm-2024-11-26",
        url: "https://haskell-wasm.github.io/pandoc-wasm/pandoc.wasm",
        sha256: "48d9ceed3ef805f6acc28e6f58c2439cdeb1f71864244fffcc155e2c045aa7fc",
        kind: ArtifactKind::File,
    },
    description: "pandoc: convert documents between Markdown, HTML, DOCX, ODT, EPUB, Typst and other formats; no network access or direct PDF engine.",
    network: NetworkAccess::Blocked,
    limits: ExecutionLimits {
        max_runtime: Duration::from_secs(60),
        max_memory_bytes: Some(1024 * 1024 * 1024),
        max_cpu_fuel: None,
    },
};

#[async_trait::async_trait]
pub trait NativeCommand: Send + Sync {
    async fn run(
        &self,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
    ) -> anyhow::Result<CommandOutput>;
}

#[derive(Clone)]
pub enum CommandHandler {
    Native(Arc<dyn NativeCommand>),
    Wasi {
        runner: Arc<WasiCommandRunner>,
        command: PreparedCommand,
    },
}

struct RegisteredCommand {
    description: &'static str,
    handler: CommandHandler,
}

pub struct CommandRouter {
    workspace: Arc<ExecutionWorkspace>,
    commands: BTreeMap<&'static str, RegisteredCommand>,
}

struct LazyWasiCommand {
    runner: Arc<WasiCommandRunner>,
    store: ArtifactStore,
    spec: CommandSpec,
    prepared: tokio::sync::OnceCell<PreparedCommand>,
}

#[async_trait::async_trait]
impl NativeCommand for LazyWasiCommand {
    async fn run(
        &self,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
    ) -> anyhow::Result<CommandOutput> {
        let command = self
            .prepared
            .get_or_try_init(|| self.runner.prepare(&self.store, &self.spec))
            .await?;
        let mut invocation = invocation.clone();
        invocation.timeout = invocation.timeout.or(Some(self.spec.limits.max_runtime));
        self.runner
            .run_with_policy(
                command,
                &invocation,
                mounts,
                &self.spec.limits,
                self.spec.network.clone(),
            )
            .await
    }
}

impl CommandRouter {
    pub fn new(workspace: Arc<ExecutionWorkspace>) -> Self {
        Self {
            workspace,
            commands: BTreeMap::new(),
        }
    }

    pub fn register_native(
        &mut self,
        name: &'static str,
        description: &'static str,
        command: Arc<dyn NativeCommand>,
    ) {
        self.commands.insert(
            name,
            RegisteredCommand {
                description,
                handler: CommandHandler::Native(command),
            },
        );
    }

    pub fn register_wasi(
        &mut self,
        name: &'static str,
        description: &'static str,
        runner: Arc<WasiCommandRunner>,
        command: PreparedCommand,
    ) {
        self.commands.insert(
            name,
            RegisteredCommand {
                description,
                handler: CommandHandler::Wasi { runner, command },
            },
        );
    }

    pub fn register_wasi_spec(
        &mut self,
        spec: CommandSpec,
        runner: Arc<WasiCommandRunner>,
        store: ArtifactStore,
    ) {
        let command = LazyWasiCommand {
            runner,
            store,
            spec: spec.clone(),
            prepared: tokio::sync::OnceCell::new(),
        };
        self.register_native(spec.name, spec.description, Arc::new(command));
    }

    pub fn catalog(&self) -> Vec<(&'static str, &'static str)> {
        self.commands
            .iter()
            .map(|(name, command)| (*name, command.description))
            .collect()
    }

    pub async fn exec(&self, invocation: ExecInvocation) -> CommandOutput {
        let Some(name) = invocation.argv.first().cloned() else {
            return unknown_command("", self.commands.keys().copied());
        };
        let Some(command) = self.commands.get(name.as_str()) else {
            return unknown_command(&name, self.commands.keys().copied());
        };
        let handler = command.handler.clone();
        let volumes = self.workspace.volumes();
        let result = self
            .workspace
            .execute(|| async move {
                match handler {
                    CommandHandler::Native(command) => command.run(&invocation, &volumes).await,
                    CommandHandler::Wasi { runner, command } => {
                        runner.run(&command, &invocation, &volumes).await
                    }
                }
            })
            .await;
        result.unwrap_or_else(|error| CommandOutput {
            returncode: 1,
            stderr: format!("{name}: {error:#}\n").into_bytes(),
            ..Default::default()
        })
    }
}

fn unknown_command<'a>(name: &str, available: impl Iterator<Item = &'a str>) -> CommandOutput {
    let available = available.collect::<Vec<_>>().join(", ");
    CommandOutput {
        returncode: 127,
        stderr: format!("unknown command: {name}\navailable commands: {available}\n").into_bytes(),
        ..Default::default()
    }
}

pub struct WasiCommandRunner {
    engine: Engine,
    cache_dir: PathBuf,
    ticker_stop: Arc<AtomicBool>,
    hub: Arc<ExecutionHub>,
}

impl WasiCommandRunner {
    pub fn new(cache_dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        Self::with_threads(cache_dir, 1)
    }

    pub fn with_threads(cache_dir: impl Into<PathBuf>, threads: usize) -> anyhow::Result<Self> {
        let mut config = Config::new();
        config
            .epoch_interruption(true)
            .consume_fuel(true)
            .strategy(Strategy::Winch)
            .generate_address_map(false);
        let engine =
            Engine::new(&config).map_err(|err| anyhow::anyhow!("create WASI engine: {err:#}"))?;
        let ticker_stop = Arc::new(AtomicBool::new(false));
        let ticker_engine = engine.clone();
        let ticker_flag = ticker_stop.clone();
        std::thread::Builder::new()
            .name("stride-wasi-epoch".to_string())
            .spawn(move || {
                while !ticker_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(EPOCH_TICK);
                    ticker_engine.increment_epoch();
                }
            })?;
        Ok(Self {
            engine,
            cache_dir: cache_dir.into(),
            ticker_stop,
            hub: execution_hub(threads),
        })
    }

    pub async fn prepare_file(
        &self,
        name: &str,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<PreparedCommand> {
        let bytes = tokio::fs::read(path).await?;
        let is_component = wasmparser::Parser::is_component(&bytes);
        anyhow::ensure!(
            is_component || wasmparser::Parser::is_core_wasm(&bytes),
            "command artifact is not WebAssembly"
        );
        let kind_name = if is_component { "p2" } else { "p1" };
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let cache_path = self.cache_dir.join(format!(
            "{name}-{kind_name}-v{COMMAND_CACHE_VERSION}-{digest}.cwasm"
        ));
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        if tokio::fs::try_exists(&cache_path).await? {
            let engine = self.engine.clone();
            let cached = cache_path.clone();
            let load = self
                .hub
                .run(move |_| -> anyhow::Result<PreparedCommandKind> {
                    // SAFETY: this runner writes the content-addressed compiled image.
                    if is_component {
                        unsafe { Component::deserialize_file(&engine, cached) }
                            .map(PreparedCommandKind::P2)
                            .map_err(|error| anyhow::anyhow!("{error:#}"))
                    } else {
                        unsafe { Module::deserialize_file(&engine, cached) }
                            .map(PreparedCommandKind::P1)
                            .map_err(|error| anyhow::anyhow!("{error:#}"))
                    }
                })
                .await?;
            if let Ok(kind) = load {
                return Ok(PreparedCommand { kind });
            }
            let _ = tokio::fs::remove_file(&cache_path).await;
        }

        let engine = self.engine.clone();
        let (kind, serialized) = self
            .hub
            .run(move |_| {
                if is_component {
                    let component = Component::new(&engine, bytes)
                        .map_err(|error| anyhow::anyhow!("compile WASI command: {error:#}"))?;
                    let serialized = component
                        .serialize()
                        .map_err(|error| anyhow::anyhow!("serialize WASI command: {error:#}"))?;
                    Ok::<_, anyhow::Error>((PreparedCommandKind::P2(component), serialized))
                } else {
                    let module = Module::new(&engine, bytes)
                        .map_err(|error| anyhow::anyhow!("compile WASI command: {error:#}"))?;
                    let serialized = module
                        .serialize()
                        .map_err(|error| anyhow::anyhow!("serialize WASI command: {error:#}"))?;
                    Ok((PreparedCommandKind::P1(module), serialized))
                }
            })
            .await??;
        let staging = cache_path.with_extension("cwasm.tmp");
        tokio::fs::write(&staging, serialized).await?;
        tokio::fs::rename(staging, cache_path).await?;
        Ok(PreparedCommand { kind })
    }

    pub async fn prepare(
        &self,
        store: &ArtifactStore,
        spec: &CommandSpec,
    ) -> anyhow::Result<PreparedCommand> {
        let directory = store.ensure(&spec.artifact).await?;
        let file_name = spec
            .artifact
            .url
            .rsplit('/')
            .next()
            .ok_or_else(|| anyhow::anyhow!("artifact URL has no file name"))?;
        self.prepare_file(spec.name, directory.join(file_name))
            .await
    }

    pub async fn run(
        &self,
        command: &PreparedCommand,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
    ) -> anyhow::Result<CommandOutput> {
        self.run_with_limits(command, invocation, mounts, &ExecutionLimits::default())
            .await
    }

    pub async fn run_with_limits(
        &self,
        command: &PreparedCommand,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
        limits: &ExecutionLimits,
    ) -> anyhow::Result<CommandOutput> {
        self.run_with_policy(command, invocation, mounts, limits, NetworkAccess::Blocked)
            .await
    }

    async fn run_with_policy(
        &self,
        command: &PreparedCommand,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
        limits: &ExecutionLimits,
        network: NetworkAccess,
    ) -> anyhow::Result<CommandOutput> {
        let engine = self.engine.clone();
        let command = command.clone();
        let invocation = invocation.clone();
        let mounts = mounts.to_vec();
        let limits = limits.clone();
        self.hub
            .run(move |_| match &command.kind {
                PreparedCommandKind::P1(_) => {
                    run_p1(&engine, &command, &invocation, &mounts, &limits)
                }
                PreparedCommandKind::P2(_) => {
                    run_p2(&engine, &command, &invocation, &mounts, &limits, &network)
                }
            })
            .await?
    }
}

impl Drop for WasiCommandRunner {
    fn drop(&mut self) {
        self.ticker_stop.store(true, Ordering::Relaxed);
    }
}

fn run_p1(
    engine: &Engine,
    command: &PreparedCommand,
    invocation: &ExecInvocation,
    mounts: &[VolumeMount],
    limits: &ExecutionLimits,
) -> anyhow::Result<CommandOutput> {
    let PreparedCommandKind::P1(module) = &command.kind else {
        anyhow::bail!("expected WASIp1 command")
    };
    let stdin = MemoryInputPipe::new(Bytes::from(invocation.stdin.clone()));
    let stdout = MemoryOutputPipe::new(MAX_OUTPUT_BYTES);
    let stderr = MemoryOutputPipe::new(MAX_OUTPUT_BYTES);
    let mut builder = WasiCtxBuilder::new();
    builder
        .stdin(stdin)
        .stdout(stdout.clone())
        .stderr(stderr.clone())
        .args(&invocation.argv);
    let _scratch = configure_preopens(&mut builder, mounts, &invocation.cwd)?;

    let mut linker = Linker::new(engine);
    p1::add_to_linker_sync(&mut linker, |state: &mut P1State| &mut state.wasi)?;
    let mut limit_builder = StoreLimitsBuilder::new().trap_on_grow_failure(true);
    if let Some(max_memory) = limits.max_memory_bytes {
        limit_builder = limit_builder.memory_size(max_memory as usize);
    }
    let mut store = Store::new(
        engine,
        P1State {
            wasi: builder.build_p1(),
            limits: limit_builder.build(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_fuel(limits.max_cpu_fuel.unwrap_or(u64::MAX))?;
    let deadline = invocation
        .timeout
        .map(|timeout| timeout.as_millis().div_ceil(EPOCH_TICK.as_millis()) as u64)
        .unwrap_or(u64::MAX / 2)
        .max(1);
    store.set_epoch_deadline(deadline);
    store.epoch_deadline_trap();
    let instance = linker.instantiate(&mut store, module)?;
    let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
    let returncode = match start.call(&mut store, ()) {
        Ok(()) => 0,
        Err(err) => {
            if matches!(
                err.downcast_ref::<wasmtime::Trap>(),
                Some(wasmtime::Trap::Interrupt)
            ) {
                return Err(anyhow::anyhow!("WASI command timed out"));
            }
            match err.downcast_ref::<wasmtime_wasi::I32Exit>() {
                Some(exit) => exit.0,
                None => return Err(anyhow::anyhow!("run WASI command: {err}")),
            }
        }
    };

    Ok(CommandOutput {
        returncode,
        stdout: stdout.contents().to_vec(),
        stderr: stderr.contents().to_vec(),
        truncated: false,
    })
}

struct P1State {
    wasi: p1::WasiP1Ctx,
    limits: StoreLimits,
}

fn run_p2(
    engine: &Engine,
    command: &PreparedCommand,
    invocation: &ExecInvocation,
    mounts: &[VolumeMount],
    limits: &ExecutionLimits,
    network: &NetworkAccess,
) -> anyhow::Result<CommandOutput> {
    let PreparedCommandKind::P2(component) = &command.kind else {
        anyhow::bail!("expected WASIp2 command")
    };
    let stdin = MemoryInputPipe::new(Bytes::from(invocation.stdin.clone()));
    let stdout = MemoryOutputPipe::new(MAX_OUTPUT_BYTES);
    let stderr = MemoryOutputPipe::new(MAX_OUTPUT_BYTES);
    let mut builder = WasiCtxBuilder::new();
    builder
        .stdin(stdin)
        .stdout(stdout.clone())
        .stderr(stderr.clone())
        .args(&invocation.argv);
    if matches!(network, NetworkAccess::Allowed) {
        builder.inherit_network();
    }
    let _scratch = configure_preopens(&mut builder, mounts, &invocation.cwd)?;

    let mut linker = ComponentLinker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    let mut limit_builder = StoreLimitsBuilder::new().trap_on_grow_failure(true);
    if let Some(max_memory) = limits.max_memory_bytes {
        limit_builder = limit_builder.memory_size(max_memory as usize);
    }
    let mut store = Store::new(
        engine,
        P2State {
            wasi: builder.build(),
            table: ResourceTable::new(),
            limits: limit_builder.build(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_fuel(limits.max_cpu_fuel.unwrap_or(u64::MAX))?;
    let deadline = invocation
        .timeout
        .map(|timeout| timeout.as_millis().div_ceil(EPOCH_TICK.as_millis()) as u64)
        .unwrap_or(u64::MAX / 2)
        .max(1);
    store.set_epoch_deadline(deadline);
    store.epoch_deadline_trap();
    let command = P2Command::instantiate(&mut store, component, &linker)?;
    let returncode = match command.wasi_cli_run().call_run(&mut store) {
        Ok(Ok(())) => 0,
        Ok(Err(())) => 1,
        Err(error) => {
            if matches!(
                error.downcast_ref::<wasmtime::Trap>(),
                Some(wasmtime::Trap::Interrupt)
            ) {
                return Err(anyhow::anyhow!("WASI command timed out"));
            }
            match error.downcast_ref::<wasmtime_wasi::I32Exit>() {
                Some(exit) => exit.0,
                None => return Err(anyhow::anyhow!("run WASI command: {error}")),
            }
        }
    };
    Ok(CommandOutput {
        returncode,
        stdout: stdout.contents().to_vec(),
        stderr: stderr.contents().to_vec(),
        truncated: false,
    })
}

struct P2State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for P2State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn configure_preopens(
    builder: &mut WasiCtxBuilder,
    mounts: &[VolumeMount],
    cwd: &str,
) -> anyhow::Result<tempfile::TempDir> {
    for mount in mounts {
        let (dir_perms, file_perms) = mount_permissions(mount.read_only);
        builder.preopened_dir(&mount.host_path, &mount.guest_path, dir_perms, file_perms)?;
    }
    if let Some((mount, relative)) = mounts
        .iter()
        .filter_map(|mount| {
            Path::new(cwd)
                .strip_prefix(&mount.guest_path)
                .ok()
                .map(|relative| (mount, relative))
        })
        .max_by_key(|(mount, _)| mount.guest_path.len())
    {
        let (dir_perms, file_perms) = mount_permissions(mount.read_only);
        builder.preopened_dir(mount.host_path.join(relative), ".", dir_perms, file_perms)?;
    }
    let scratch = tempfile::tempdir()?;
    builder.preopened_dir(
        scratch.path(),
        "/tmp",
        wasmtime_wasi::DirPerms::all(),
        wasmtime_wasi::FilePerms::all(),
    )?;
    Ok(scratch)
}

fn mount_permissions(read_only: bool) -> (wasmtime_wasi::DirPerms, wasmtime_wasi::FilePerms) {
    if read_only {
        (
            wasmtime_wasi::DirPerms::READ,
            wasmtime_wasi::FilePerms::READ,
        )
    } else {
        (
            wasmtime_wasi::DirPerms::all(),
            wasmtime_wasi::FilePerms::all(),
        )
    }
}
