//! Bashkit `typst` builtin: a Typst document compiler for the emulated shell.
//!
//! The sandbox has no real shell to spawn a `typst` binary, so this runs the
//! embedded Rust compiler directly. It gathers the project from the shell's
//! virtual filesystem, compiles in memory and writes the output back to the
//! VFS:
//!
//! ```bash
//! typst compile report.typ                 # -> report.pdf
//! typst compile report.typ out.png --ppi 300
//! typst compile report.typ --format svg --root /home/agent
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bashkit::{Builtin, BuiltinContext, ExecResult, async_trait};

use crate::typst_doc::{self, CompileRequest, TypstFormat};
#[cfg(feature = "eryx")]
use crate::{CommandOutput, ExecInvocation, NativeCommand, VolumeMount};

/// Refuse to slurp a pathologically large project tree into memory.
const MAX_PROJECT_BYTES: usize = 64 * 1024 * 1024;

/// Hard wall-clock bound on a single compile.
const MAX_COMPILE_TIME: std::time::Duration = std::time::Duration::from_secs(60);

const USAGE: &str = "usage: typst <command> [options]\n\
     Commands:\n  \
     compile <input.typ> [output] [--format pdf|svg|png] [--ppi N] [--root DIR] [--input k=v]\n  \
     query   <input.typ> <selector>   (not supported yet)\n  \
     Options:\n  \
     -f, --format   output format (default: inferred from output, else pdf)\n  \
     --ppi          pixels per inch for png output (default: 144)\n  \
     --root         project root directory (default: the input's directory)\n  \
     --input k=v    expose a value through sys.inputs\n  \
     -V, --version  print version\n";

pub const TYPST_DESCRIPTION: &str = "typst: compile a Typst document from the workspace to PDF/SVG/PNG \
     (typst compile report.typ [out.pdf] [--format pdf|svg|png] [--ppi N] \
     [--root DIR]). Fonts are bundled; @preview packages download when \
     network is enabled. Writes the output file into the workspace.";

/// Compiles Typst documents from the workspace. Construct with the package
/// cache directory and whether network package downloads are allowed.
pub struct TypstBuiltin {
    package_cache: Option<PathBuf>,
    font_paths: Vec<PathBuf>,
    allow_network: bool,
}

impl TypstBuiltin {
    pub fn new(
        package_cache: Option<PathBuf>,
        font_paths: Vec<PathBuf>,
        allow_network: bool,
    ) -> Self {
        Self {
            package_cache,
            font_paths,
            allow_network,
        }
    }
}

#[cfg(feature = "eryx")]
pub struct TypstCommand {
    package_cache: Option<PathBuf>,
    font_paths: Vec<PathBuf>,
    allow_network: bool,
}

#[cfg(feature = "eryx")]
impl TypstCommand {
    pub fn new(
        package_cache: Option<PathBuf>,
        font_paths: Vec<PathBuf>,
        allow_network: bool,
    ) -> Self {
        Self {
            package_cache,
            font_paths,
            allow_network,
        }
    }

    async fn compile(
        &self,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
    ) -> Result<CommandOutput, String> {
        let options = CompileArgs::parse(&invocation.argv[2..])?;
        let cwd = Path::new(&invocation.cwd);
        let input_guest = normalize_guest_path(&resolve_path(cwd, &options.input))?;
        let root_guest = match &options.root {
            Some(root) => normalize_guest_path(&resolve_path(cwd, root))?,
            None => input_guest
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
        };
        let Some(main_rel) = relative_key(&input_guest, &root_guest) else {
            return Err(format!(
                "input '{}' is not inside root '{}'",
                input_guest.display(),
                root_guest.display()
            ));
        };
        let root_host = resolve_host_path(&root_guest, mounts, false)?;
        let files = gather_host_project(&root_host).await?;
        if !files.contains_key(&main_rel) {
            return Err(format!("can't open input file '{}'", input_guest.display()));
        }

        let output_guest = options
            .output
            .as_ref()
            .map(|output| normalize_guest_path(&resolve_path(cwd, output)))
            .transpose()?
            .unwrap_or_else(|| {
                default_output(&input_guest, options.format.unwrap_or(TypstFormat::Pdf))
            });
        let format = options
            .format
            .or_else(|| format_of(&output_guest))
            .unwrap_or(TypstFormat::Pdf);
        let output_host = resolve_host_path(&output_guest, mounts, true)?;
        let request = CompileRequest {
            files,
            main: main_rel,
            format,
            ppi: options.ppi.unwrap_or(typst_doc::DEFAULT_PPI),
            sys_inputs: options.inputs,
            package_cache: self.package_cache.clone(),
            font_paths: self.font_paths.clone(),
            allow_network: self.allow_network,
        };
        let compile = tokio::task::spawn_blocking(move || typst_doc::compile(request));
        let bytes = match tokio::time::timeout(MAX_COMPILE_TIME, compile).await {
            Ok(Ok(Ok(bytes))) => bytes,
            Ok(Ok(Err(error))) => return Err(format!("{error:#}")),
            Ok(Err(error)) => return Err(format!("compiler panicked: {error}")),
            Err(_) => return Err("compilation timed out".to_string()),
        };
        if let Some(parent) = output_host.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| format!("can't create '{}': {error}", parent.display()))?;
        }
        tokio::fs::write(&output_host, &bytes)
            .await
            .map_err(|error| format!("can't write '{}': {error}", output_guest.display()))?;
        Ok(CommandOutput {
            stdout: format!(
                "Compiled {} -> {} ({} bytes)\n",
                input_guest.display(),
                output_guest.display(),
                bytes.len()
            )
            .into_bytes(),
            ..Default::default()
        })
    }
}

#[cfg(feature = "eryx")]
#[async_trait]
impl NativeCommand for TypstCommand {
    async fn run(
        &self,
        invocation: &ExecInvocation,
        mounts: &[VolumeMount],
    ) -> anyhow::Result<CommandOutput> {
        let response = match invocation.argv.get(1).map(String::as_str) {
            Some("--version" | "-V") => CommandOutput {
                stdout: b"typst 0.15.0 (execenv)\n".to_vec(),
                ..Default::default()
            },
            Some("--help" | "-h") | None => CommandOutput {
                stdout: USAGE.as_bytes().to_vec(),
                ..Default::default()
            },
            Some("compile" | "c") => match self.compile(invocation, mounts).await {
                Ok(output) => output,
                Err(error) => CommandOutput {
                    returncode: 1,
                    stderr: format!("typst: {error}\n").into_bytes(),
                    ..Default::default()
                },
            },
            Some("query" | "q") => CommandOutput {
                returncode: 1,
                stderr: b"typst: query is not supported yet\n".to_vec(),
                ..Default::default()
            },
            Some(other) => CommandOutput {
                returncode: 2,
                stderr: format!("typst: unknown command '{other}'\n{USAGE}").into_bytes(),
                ..Default::default()
            },
        };
        Ok(response)
    }
}

#[async_trait]
impl Builtin for TypstBuiltin {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        match ctx.args.first().map(String::as_str) {
            Some("--version" | "-V") => Ok(ExecResult::ok("typst 0.15.0 (execenv)\n")),
            Some("--help" | "-h") | None => Ok(ExecResult::ok(USAGE)),
            Some("compile" | "c") => self.run_compile(&ctx).await,
            Some("query" | "q") => Ok(ExecResult::err(
                "typst: query is not supported yet\n".to_string(),
                1,
            )),
            Some(other) => Ok(ExecResult::err(
                format!("typst: unknown command '{other}'\n{USAGE}"),
                2,
            )),
        }
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(TYPST_DESCRIPTION)
    }
}

impl TypstBuiltin {
    async fn run_compile(&self, ctx: &BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let options = match CompileArgs::parse(&ctx.args[1..]) {
            Ok(options) => options,
            Err(err) => return Ok(ExecResult::err(format!("typst: {err}\n"), 2)),
        };

        let input_abs = resolve_path(ctx.cwd, &options.input);
        let root_abs = match &options.root {
            Some(root) => resolve_path(ctx.cwd, root),
            None => input_abs
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
        };

        let Some(main_rel) = relative_key(&input_abs, &root_abs) else {
            return Ok(ExecResult::err(
                format!(
                    "typst: input '{}' is not inside root '{}'\n",
                    input_abs.display(),
                    root_abs.display()
                ),
                2,
            ));
        };

        let files = match gather_project(ctx.fs.clone(), &root_abs).await {
            Ok(files) => files,
            Err(err) => return Ok(ExecResult::err(format!("typst: {err}\n"), 1)),
        };
        if !files.contains_key(&main_rel) {
            return Ok(ExecResult::err(
                format!("typst: can't open input file '{}'\n", input_abs.display()),
                2,
            ));
        }

        let output_abs = options
            .output
            .as_ref()
            .map(|output| resolve_path(ctx.cwd, output));
        let format = options
            .format
            .or_else(|| output_abs.as_ref().and_then(|p| format_of(p)))
            .unwrap_or(TypstFormat::Pdf);
        let output_abs = output_abs.unwrap_or_else(|| default_output(&input_abs, format));

        let request = CompileRequest {
            files,
            main: main_rel,
            format,
            ppi: options.ppi.unwrap_or(typst_doc::DEFAULT_PPI),
            sys_inputs: options.inputs,
            package_cache: self.package_cache.clone(),
            font_paths: self.font_paths.clone(),
            allow_network: self.allow_network,
        };

        let compile = tokio::task::spawn_blocking(move || typst_doc::compile(request));
        let bytes = match tokio::time::timeout(MAX_COMPILE_TIME, compile).await {
            Ok(Ok(Ok(bytes))) => bytes,
            Ok(Ok(Err(err))) => return Ok(ExecResult::err(format!("typst: {err:#}\n"), 1)),
            Ok(Err(err)) => {
                return Ok(ExecResult::err(
                    format!("typst: compiler panicked: {err}\n"),
                    1,
                ));
            }
            Err(_) => {
                return Ok(ExecResult::err(
                    "typst: compilation timed out\n".to_string(),
                    1,
                ));
            }
        };

        if let Some(parent) = output_abs.parent()
            && !parent.as_os_str().is_empty()
            && ctx.fs.mkdir(parent, true).await.is_err()
        {
            // Non-fatal: write_file fails below with a clearer message.
        }
        let written = bytes.len();
        if let Err(err) = ctx.fs.write_file(&output_abs, &bytes).await {
            return Ok(ExecResult::err(
                format!("typst: can't write '{}': {err}\n", output_abs.display()),
                1,
            ));
        }

        Ok(ExecResult::ok(format!(
            "Compiled {} -> {} ({written} bytes)\n",
            input_abs.display(),
            output_abs.display()
        )))
    }
}

/// Parsed `typst compile` arguments.
struct CompileArgs {
    input: String,
    output: Option<String>,
    format: Option<TypstFormat>,
    ppi: Option<f32>,
    root: Option<String>,
    inputs: BTreeMap<String, String>,
}

impl CompileArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut positionals = Vec::new();
        let mut format = None;
        let mut ppi = None;
        let mut root = None;
        let mut inputs = BTreeMap::new();

        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-f" | "--format" => {
                    let value = iter.next().ok_or("option --format requires an argument")?;
                    format = Some(
                        TypstFormat::parse(value)
                            .ok_or_else(|| format!("unknown format '{value}'"))?,
                    );
                }
                "--ppi" => {
                    let value = iter.next().ok_or("option --ppi requires an argument")?;
                    ppi = Some(
                        value
                            .parse::<f32>()
                            .map_err(|_| format!("invalid ppi '{value}'"))?,
                    );
                }
                "--root" => {
                    root = Some(
                        iter.next()
                            .ok_or("option --root requires an argument")?
                            .clone(),
                    );
                }
                "--input" => {
                    let value = iter.next().ok_or("option --input requires an argument")?;
                    let (key, val) = value.split_once('=').ok_or("--input expects key=value")?;
                    inputs.insert(key.to_string(), val.to_string());
                }
                other if other.starts_with('-') && other != "-" => {
                    return Err(format!("unknown option '{other}'"));
                }
                _ => positionals.push(arg.clone()),
            }
        }

        let mut positionals = positionals.into_iter();
        let input = positionals.next().ok_or("missing input file")?;
        let output = positionals.next();
        if positionals.next().is_some() {
            return Err("too many arguments".to_string());
        }

        Ok(Self {
            input,
            output,
            format,
            ppi,
            root,
            inputs,
        })
    }
}

/// Recursively reads every file under `root` into a project map keyed by path
/// relative to `root`, using forward slashes.
async fn gather_project(
    fs: Arc<dyn bashkit::FileSystem>,
    root: &Path,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = fs
            .read_dir(&dir)
            .await
            .map_err(|err| format!("can't read directory '{}': {err}", dir.display()))?;
        for entry in entries {
            let path = dir.join(&entry.name);
            if entry.metadata.file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = fs
                .read_file(&path)
                .await
                .map_err(|err| format!("can't read '{}': {err}", path.display()))?;
            total = total.saturating_add(bytes.len());
            if total > MAX_PROJECT_BYTES {
                return Err("project is too large to compile".to_string());
            }
            if let Some(key) = relative_key(&path, root) {
                files.insert(key, bytes);
            }
        }
    }

    Ok(files)
}

#[cfg(feature = "eryx")]
async fn gather_host_project(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&directory)
            .await
            .map_err(|error| format!("can't read directory '{}': {error}", directory.display()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| format!("can't read directory '{}': {error}", directory.display()))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|error| format!("can't inspect '{}': {error}", entry.path().display()))?;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                let bytes = tokio::fs::read(entry.path())
                    .await
                    .map_err(|error| format!("can't read '{}': {error}", entry.path().display()))?;
                total = total.saturating_add(bytes.len());
                if total > MAX_PROJECT_BYTES {
                    return Err("project is too large to compile".to_string());
                }
                if let Some(key) = relative_key(&entry.path(), root) {
                    files.insert(key, bytes);
                }
            }
        }
    }
    Ok(files)
}

#[cfg(feature = "eryx")]
fn normalize_guest_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!("path escapes sandbox: '{}'", path.display()));
                }
            }
            std::path::Component::Prefix(_) => {
                return Err(format!("invalid guest path: '{}'", path.display()));
            }
        }
    }
    Ok(normalized)
}

#[cfg(feature = "eryx")]
fn resolve_host_path(guest: &Path, mounts: &[VolumeMount], write: bool) -> Result<PathBuf, String> {
    let mount = mounts
        .iter()
        .filter_map(|mount| {
            let guest_root = Path::new(&mount.guest_path);
            guest
                .strip_prefix(guest_root)
                .ok()
                .map(|relative| (mount, relative))
        })
        .max_by_key(|(mount, _)| mount.guest_path.len())
        .ok_or_else(|| format!("path is not mounted: '{}'", guest.display()))?;
    if write && mount.0.read_only {
        return Err(format!("path is read-only: '{}'", guest.display()));
    }
    Ok(mount.0.host_path.join(mount.1))
}

/// Path of `path` relative to `root`, as a forward-slash string. `None` if
/// `path` is not under `root`.
fn relative_key(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut key = String::new();
    for component in rel.components() {
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(&component.as_os_str().to_string_lossy());
    }
    (!key.is_empty()).then_some(key)
}

fn format_of(path: &Path) -> Option<TypstFormat> {
    path.extension()
        .and_then(|ext| TypstFormat::parse(&ext.to_string_lossy()))
}

fn default_output(input: &Path, format: TypstFormat) -> PathBuf {
    input.with_extension(format.extension())
}

fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bashkit::{Bash, FileSystem, InMemoryFs};

    use super::*;

    fn bash_with_typst(fs: Arc<dyn FileSystem>) -> Bash {
        Bash::builder()
            .fs(fs)
            .cwd("/home/agent")
            .builtin(
                "typst",
                Box::new(TypstBuiltin::new(None, Vec::new(), false)),
            )
            .build()
    }

    async fn workspace(main: &str) -> Arc<InMemoryFs> {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(Path::new("/home/agent"), true).await.unwrap();
        fs.write_file(Path::new("/home/agent/report.typ"), main.as_bytes())
            .await
            .unwrap();
        fs
    }

    #[tokio::test]
    async fn compiles_to_default_pdf() {
        let fs = workspace("= Title\nBody").await;
        let mut bash = bash_with_typst(fs.clone());
        let result = bash.exec("typst compile report.typ").await.unwrap();
        assert_eq!(result.exit_code, 0, "stderr={}", result.stderr);
        let pdf = fs
            .read_file(Path::new("/home/agent/report.pdf"))
            .await
            .unwrap();
        assert_eq!(&pdf[..5], b"%PDF-");
    }

    #[tokio::test]
    async fn compiles_png_with_explicit_output() {
        let fs = workspace("Hello").await;
        let mut bash = bash_with_typst(fs.clone());
        let result = bash
            .exec("typst compile report.typ out.png --ppi 200")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "stderr={}", result.stderr);
        let png = fs
            .read_file(Path::new("/home/agent/out.png"))
            .await
            .unwrap();
        assert_eq!(&png[..4], &[0x89, 0x50, 0x4e, 0x47]);
    }

    #[tokio::test]
    async fn format_flag_overrides() {
        let fs = workspace("Hello").await;
        let mut bash = bash_with_typst(fs.clone());
        let result = bash
            .exec("typst compile report.typ doc.out --format svg")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "stderr={}", result.stderr);
        let svg = fs
            .read_file(Path::new("/home/agent/doc.out"))
            .await
            .unwrap();
        assert!(svg.starts_with(b"<svg") || svg.windows(4).any(|w| w == b"<svg"));
    }

    #[tokio::test]
    async fn resolves_local_import() {
        let fs = workspace("#import \"inc.typ\": who\nHello #who").await;
        fs.write_file(Path::new("/home/agent/inc.typ"), b"#let who = [there]")
            .await
            .unwrap();
        let mut bash = bash_with_typst(fs.clone());
        let result = bash.exec("typst compile report.typ").await.unwrap();
        assert_eq!(result.exit_code, 0, "stderr={}", result.stderr);
    }

    #[tokio::test]
    async fn missing_input_fails() {
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(Path::new("/home/agent"), true).await.unwrap();
        let mut bash = bash_with_typst(fs);
        let result = bash.exec("typst compile nope.typ").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(
            result.stderr.contains("can't open input file"),
            "{}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn syntax_error_is_reported() {
        let fs = workspace("#let = 1").await;
        let mut bash = bash_with_typst(fs);
        let result = bash.exec("typst compile report.typ").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("error"), "{}", result.stderr);
    }

    #[tokio::test]
    async fn version_reported() {
        let fs = Arc::new(InMemoryFs::new());
        let mut bash = bash_with_typst(fs);
        let result = bash.exec("typst --version").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("execenv"));
    }
}
