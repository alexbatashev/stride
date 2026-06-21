//! Exposes the embedded Typst compiler to the in-sandbox Python interpreter as
//! a `typst` module.
//!
//! A host [`eryx::Callback`] (`__friday_typst_compile`) runs the native compiler
//! on the host; the [`PREAMBLE`] defines a Python `typst` module that gathers
//! project files from the sandbox filesystem, ships them to the callback and
//! decodes the returned bytes. The module is injected on every execution, so
//! `import typst` works from the `python` tool and the shell `python` builtin
//! alike. The host call is asynchronous, so the API is awaited:
//! `await typst.compile("doc.typ", output="doc.pdf")`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use serde_json::{Value, json};

use crate::typst_doc::{self, CompileRequest, TypstFormat};

/// Name the Python preamble uses to reach the host compile callback.
pub(crate) const CALLBACK_NAME: &str = "__friday_typst_compile";

/// Hard wall-clock bound on a single host-side compile.
const MAX_COMPILE_TIME: std::time::Duration = std::time::Duration::from_secs(60);

/// Cap on the total decoded size of project files accepted from the guest.
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

/// Cap on the size of a compiled artifact returned to the guest.
const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;

fn base64_engine() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Builds the host-side Typst compile callback for one execution.
pub(crate) fn callback(
    package_cache: Option<PathBuf>,
    font_paths: Vec<PathBuf>,
    allow_network: bool,
) -> Arc<dyn eryx::Callback> {
    let schema = eryx::Schema::try_from_value(json!({ "type": "object" }))
        .unwrap_or_else(|_| eryx::Schema::empty());
    let callback = eryx::DynamicCallback::builder(
        CALLBACK_NAME.to_string(),
        "Compile a Typst document to PDF, SVG or PNG.".to_string(),
        move |args| {
            let package_cache = package_cache.clone();
            let font_paths = font_paths.clone();
            Box::pin(async move { Ok(run(args, package_cache, font_paths, allow_network).await) })
        },
    )
    .schema(schema)
    .build();
    Arc::new(callback)
}

async fn run(
    args: Value,
    package_cache: Option<PathBuf>,
    font_paths: Vec<PathBuf>,
    allow_network: bool,
) -> Value {
    let request = match parse_request(args, package_cache, font_paths, allow_network) {
        Ok(request) => request,
        Err(err) => return json!({ "ok": false, "error": err }),
    };
    // The compiler is untrusted-input driven; bound each compile so a malicious
    // document cannot tie up a worker thread indefinitely.
    let compile = tokio::task::spawn_blocking(move || typst_doc::compile(request));
    let bytes = match tokio::time::timeout(MAX_COMPILE_TIME, compile).await {
        Ok(Ok(Ok(bytes))) => bytes,
        Ok(Ok(Err(err))) => return json!({ "ok": false, "error": format!("{err:#}") }),
        Ok(Err(err)) => {
            return json!({ "ok": false, "error": format!("typst compiler task failed: {err}") });
        }
        Err(_) => return json!({ "ok": false, "error": "typst compilation timed out" }),
    };
    if bytes.len() > MAX_OUTPUT_BYTES {
        return json!({
            "ok": false,
            "error": format!("typst output is too large ({} bytes)", bytes.len()),
        });
    }
    json!({ "ok": true, "data": base64_engine().encode(bytes) })
}

fn parse_request(
    args: Value,
    package_cache: Option<PathBuf>,
    font_paths: Vec<PathBuf>,
    allow_network: bool,
) -> Result<CompileRequest, String> {
    let object = args.as_object().ok_or("expected an object of arguments")?;

    let files_obj = object
        .get("files")
        .and_then(Value::as_object)
        .ok_or("missing 'files'")?;
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for (path, value) in files_obj {
        let encoded = value.as_str().ok_or("each file must be a base64 string")?;
        let bytes = base64_engine()
            .decode(encoded)
            .map_err(|err| format!("invalid base64 for {path:?}: {err}"))?;
        // Enforce the size cap on the host: the Python-side limit is advisory
        // and a script could call the callback directly with a huge payload.
        total = total.saturating_add(bytes.len());
        if total > MAX_INPUT_BYTES {
            return Err("typst project is too large".to_string());
        }
        files.insert(path.clone(), bytes);
    }

    let main = object
        .get("main")
        .and_then(Value::as_str)
        .ok_or("missing 'main'")?
        .to_string();

    let format = object
        .get("format")
        .and_then(Value::as_str)
        .and_then(TypstFormat::parse)
        .unwrap_or(TypstFormat::Pdf);

    let ppi = object
        .get("ppi")
        .and_then(Value::as_f64)
        .map(|ppi| ppi as f32)
        .unwrap_or(typst_doc::DEFAULT_PPI);

    let mut sys_inputs = BTreeMap::new();
    if let Some(inputs) = object.get("sys_inputs").and_then(Value::as_object) {
        for (key, value) in inputs {
            if let Some(text) = value.as_str() {
                sys_inputs.insert(key.clone(), text.to_string());
            }
        }
    }

    Ok(CompileRequest {
        files,
        main,
        format,
        ppi,
        sys_inputs,
        package_cache,
        font_paths,
        allow_network,
    })
}

/// Python module definition injected ahead of every script. Defines `typst`
/// (in `sys.modules` and as a global) with `compile`, `query` and `Compiler`,
/// mirroring the typst-py API but awaitable.
pub(crate) const PREAMBLE: &str = r##"
import sys as _typst_sys, types as _typst_types, os as _typst_os, base64 as _typst_b64

_TYPST_CALLBACK = "__friday_typst_compile"
_TYPST_MAX_BYTES = 64 * 1024 * 1024


def _typst_gather(source, root):
    if isinstance(source, (bytes, bytearray)):
        return {"main.typ": _typst_b64.b64encode(bytes(source)).decode("ascii")}, "main.typ"
    main_path = _typst_os.path.abspath(source)
    base = _typst_os.path.abspath(root) if root else (_typst_os.path.dirname(main_path) or ".")
    files = {}
    total = 0
    for dirpath, _dirs, names in _typst_os.walk(base):
        for name in names:
            full = _typst_os.path.join(dirpath, name)
            try:
                with open(full, "rb") as handle:
                    data = handle.read()
            except OSError:
                continue
            total += len(data)
            if total > _TYPST_MAX_BYTES:
                raise RuntimeError("typst: project is too large to compile")
            rel = _typst_os.path.relpath(full, base).replace(_typst_os.sep, "/")
            files[rel] = _typst_b64.b64encode(data).decode("ascii")
    main = _typst_os.path.relpath(main_path, base).replace(_typst_os.sep, "/")
    if main.startswith("../"):
        raise RuntimeError("typst: input is not inside root")
    if main not in files:
        with open(main_path, "rb") as handle:
            files[main] = _typst_b64.b64encode(handle.read()).decode("ascii")
    return files, main


def _typst_pick_format(fmt, output):
    if fmt:
        return fmt
    if output and "." in output:
        ext = output.rsplit(".", 1)[-1].lower()
        if ext in ("pdf", "svg", "png"):
            return ext
    return "pdf"


async def _typst_compile(input, output=None, format=None, ppi=None, root=None, sys_inputs=None):
    files, main = _typst_gather(input, root)
    payload = {
        "files": files,
        "main": main,
        "format": _typst_pick_format(format, output),
        "ppi": float(ppi) if ppi is not None else 144.0,
        "sys_inputs": {str(k): str(v) for k, v in (sys_inputs or {}).items()},
    }
    try:
        result = await invoke(_TYPST_CALLBACK, **payload)
    except Exception as exc:
        raise RuntimeError("typst compilation failed: %s" % exc)
    if not isinstance(result, dict) or not result.get("ok"):
        message = result.get("error") if isinstance(result, dict) else str(result)
        raise RuntimeError(message or "typst compilation failed")
    data = _typst_b64.b64decode(result["data"])
    if output is not None:
        parent = _typst_os.path.dirname(output)
        if parent and not _typst_os.path.isdir(parent):
            try:
                _typst_os.makedirs(parent, exist_ok=True)
            except OSError:
                pass
        with open(output, "wb") as handle:
            handle.write(data)
        return None
    return data


async def _typst_query(*args, **kwargs):
    raise NotImplementedError("typst.query is not supported yet")


class _TypstCompiler:
    def __init__(self, root=None, font_paths=None, sys_inputs=None):
        self.root = root
        self.sys_inputs = sys_inputs or {}

    async def compile(self, input, output=None, format=None, ppi=None):
        return await _typst_compile(
            input, output=output, format=format, ppi=ppi, root=self.root, sys_inputs=self.sys_inputs
        )

    async def query(self, *args, **kwargs):
        raise NotImplementedError("typst.query is not supported yet")


_typst_module = _typst_types.ModuleType("typst")
_typst_module.compile = _typst_compile
_typst_module.query = _typst_query
_typst_module.Compiler = _TypstCompiler
_typst_sys.modules["typst"] = _typst_module
typst = _typst_module
"##;
