//! In-memory Typst document compiler.
//!
//! A [`CompileRequest`] carries a self-contained project — a set of virtual
//! files plus the path of the main entrypoint — which is compiled to PDF, SVG
//! or PNG using fonts bundled with the binary. `@preview` packages are fetched
//! from Typst Universe into an on-disk cache when network access is permitted
//! and otherwise served from that cache.
//!
//! The compiler runs entirely on host memory: no host filesystem path is read
//! except the package cache. Both the `typst` shell builtin and the in-sandbox
//! `typst` Python module funnel through [`compile`], so a project gathered from
//! the bashkit VFS and one shipped from the Python sandbox compile identically.

use std::collections::BTreeMap;
use std::path::PathBuf;

use typst::World;
use typst::diag::{FileError, FileResult, PackageError, Severity, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Datetime, Dict, Duration, Str, Value};
use typst::layout::Abs;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook, FontInfo};
use typst::utils::{LazyHash, Scalar};
use typst::visualize::Color;
use typst::{Library, LibraryExt};
use typst_kit::downloader::SystemDownloader;
use typst_kit::files::{FileLoader, FileStore};
use typst_kit::fonts::{FontPath, FontStore};
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;
use typst_svg::SvgOptions;

/// Default pixels-per-inch for PNG output when unspecified.
pub const DEFAULT_PPI: f32 = 144.0;

/// Upper bound on PNG ppi. A larger value is silently clamped: the rendered
/// raster grows with `ppi²`, so an unbounded value would exhaust host memory.
const MAX_PPI: f32 = 600.0;

/// Upper bound on the total size of a project handed to the compiler. Guards
/// the host against an oversized in-memory file map from any caller.
const MAX_PROJECT_BYTES: usize = 64 * 1024 * 1024;

/// Output format of a compiled document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypstFormat {
    Pdf,
    Svg,
    Png,
}

impl TypstFormat {
    /// Parses a format name (`pdf`, `svg`, `png`), case-insensitively.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            "svg" => Some(Self::Svg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    /// Infers the format from a file extension. Returns `None` for an
    /// unrecognized or missing extension (callers default to PDF).
    pub fn from_output_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?;
        Self::parse(ext)
    }

    /// The conventional file extension for this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Svg => "svg",
            Self::Png => "png",
        }
    }
}

/// A self-contained Typst project plus the desired output.
pub struct CompileRequest {
    /// Project files keyed by path relative to the project root (e.g.
    /// `doc.typ`, `images/logo.png`). Leading slashes are optional.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Key into `files` for the entrypoint that is compiled.
    pub main: String,
    pub format: TypstFormat,
    /// Pixels per inch for PNG output. Ignored for PDF and SVG.
    pub ppi: f32,
    /// Values exposed to the document through `sys.inputs`.
    pub sys_inputs: BTreeMap<String, String>,
    /// Directory used to cache downloaded Typst Universe packages. When `None`,
    /// `@preview` imports fail.
    pub package_cache: Option<PathBuf>,
    /// Directories scanned recursively for additional fonts, in addition to the
    /// fonts embedded in the binary. Used to expose the shared font cache.
    pub font_paths: Vec<PathBuf>,
    /// Whether the compiler may download missing packages from the network.
    pub allow_network: bool,
}

impl Default for CompileRequest {
    fn default() -> Self {
        Self {
            files: BTreeMap::new(),
            main: String::new(),
            format: TypstFormat::Pdf,
            ppi: DEFAULT_PPI,
            sys_inputs: BTreeMap::new(),
            package_cache: None,
            font_paths: Vec::new(),
            allow_network: false,
        }
    }
}

/// Compiles the project to the requested format and returns the encoded bytes.
///
/// This is synchronous and CPU-bound (and may block on a network package
/// download); callers on an async runtime should run it on a blocking thread.
pub fn compile(request: CompileRequest) -> anyhow::Result<Vec<u8>> {
    let total: usize = request.files.values().map(Vec::len).sum();
    anyhow::ensure!(
        total <= MAX_PROJECT_BYTES,
        "typst project is too large ({total} bytes; limit is {MAX_PROJECT_BYTES})"
    );
    if let Some(cache) = &request.package_cache {
        let _ = std::fs::create_dir_all(cache);
    }
    let world = TypstWorld::new(&request)?;
    let Warned {
        output,
        warnings: _,
    } = typst::compile::<PagedDocument>(&world);
    let document = match output {
        Ok(document) => document,
        Err(diagnostics) => {
            comemo::evict(10);
            anyhow::bail!(
                "typst compilation failed:\n{}",
                format_diagnostics(&diagnostics)
            );
        }
    };

    let bytes = encode(&request, &document);
    comemo::evict(10);
    bytes
}

/// Querying a compiled document is not supported yet. See the module-level note
/// in the change description; this returns a clear error rather than a fragile
/// partial implementation.
pub fn query(_request: CompileRequest, _selector: &str) -> anyhow::Result<String> {
    anyhow::bail!("typst query is not supported yet")
}

fn encode(request: &CompileRequest, document: &PagedDocument) -> anyhow::Result<Vec<u8>> {
    match request.format {
        TypstFormat::Pdf => {
            typst_pdf::pdf(document, &PdfOptions::default()).map_err(|diagnostics| {
                anyhow::anyhow!("PDF export failed:\n{}", format_diagnostics(&diagnostics))
            })
        }
        TypstFormat::Svg => {
            Ok(typst_svg::svg_merged(document, &SvgOptions::default(), Abs::zero()).into_bytes())
        }
        TypstFormat::Png => {
            // Clamp the (possibly untrusted) ppi: a huge value would request a
            // gigapixel raster and exhaust host memory. Non-finite or
            // non-positive values fall back to the default.
            let ppi = if request.ppi.is_finite() && request.ppi > 0.0 {
                request.ppi.min(MAX_PPI)
            } else {
                DEFAULT_PPI
            };
            let options = RenderOptions {
                pixel_per_pt: Scalar::new(f64::from(ppi) / 72.0),
                ..Default::default()
            };
            let pixmap =
                typst_render::render_merged(document, &options, Abs::zero(), Some(Color::WHITE));
            pixmap
                .encode_png()
                .map_err(|err| anyhow::anyhow!("PNG encoding failed: {err}"))
        }
    }
}

/// Renders compiler diagnostics into a human-readable block. Line numbers are
/// omitted: 0.15's `DiagSpan` does not map cheaply to source positions, and the
/// messages are descriptive on their own.
fn format_diagnostics(diagnostics: &[SourceDiagnostic]) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        let label = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        out.push_str(&format!("{label}: {}\n", diagnostic.message));
        for hint in &diagnostic.hints {
            out.push_str(&format!("  hint: {}\n", hint.v));
        }
    }
    out
}

/// A [`World`] backed by in-memory project files and bundled fonts.
struct TypstWorld {
    library: LazyHash<Library>,
    fonts: FontStore,
    files: FileStore<MemoryLoader>,
    main: FileId,
}

impl TypstWorld {
    fn new(request: &CompileRequest) -> anyhow::Result<Self> {
        let mut inputs = Dict::new();
        for (key, value) in &request.sys_inputs {
            inputs.insert(Str::from(key.clone()), Value::Str(Str::from(value.clone())));
        }
        let library = Library::builder().with_inputs(inputs).build();

        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        for dir in &request.font_paths {
            load_fonts_from_dir(dir, &mut fonts);
        }

        let mut project = BTreeMap::new();
        for (path, bytes) in &request.files {
            let key = virtual_key(path)?;
            project.insert(key, Bytes::new(bytes.clone()));
        }

        let main_key = virtual_key(&request.main)?;
        anyhow::ensure!(
            project.contains_key(&main_key),
            "main file {:?} is not present in the project",
            request.main
        );
        let main = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new(&request.main)
                .map_err(|err| anyhow::anyhow!("invalid main path {:?}: {err:?}", request.main))?,
        ));

        let packages = build_packages(request.package_cache.clone());
        let loader = MemoryLoader {
            project,
            packages,
            cache_only: !request.allow_network,
        };

        Ok(Self {
            library: LazyHash::new(library),
            fonts,
            files: FileStore::new(loader),
            main,
        })
    }
}

impl World for TypstWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        system_today()
    }
}

/// Loads project files from memory and package files from the cache (downloading
/// when permitted).
struct MemoryLoader {
    project: BTreeMap<String, Bytes>,
    packages: Option<SystemPackages>,
    cache_only: bool,
}

impl FileLoader for MemoryLoader {
    fn load(&self, id: FileId) -> FileResult<Bytes> {
        match id.root() {
            VirtualRoot::Project => self
                .project
                .get(id.vpath().get_with_slash())
                .cloned()
                .ok_or_else(|| FileError::NotFound(PathBuf::from(id.vpath().get_without_slash()))),
            VirtualRoot::Package(spec) => {
                let packages = self.packages.as_ref().ok_or_else(|| {
                    FileError::Package(PackageError::Other(Some(
                        "typst packages are unavailable: no package cache is configured".into(),
                    )))
                })?;
                let root = if self.cache_only {
                    packages
                        .cache()
                        .and_then(|cache| cache.obtain(spec))
                        .ok_or_else(|| {
                            FileError::Package(PackageError::Other(Some(
                                format!(
                                    "package {spec} is not cached and network access is disabled"
                                )
                                .into(),
                            )))
                        })?
                } else {
                    packages.obtain(spec)?
                };
                root.load(id.vpath())
            }
        }
    }
}

/// Recursively scans `dir` for font files and registers each face with the
/// store. Faces load lazily through [`FontPath`], so only fonts actually used by
/// a document are parsed into memory; metadata is read up front to build the
/// font book. Unreadable files and directories are skipped silently.
fn load_fonts_from_dir(dir: &std::path::Path, fonts: &mut FontStore) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !is_font_file(&path) {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            for (index, info) in FontInfo::iter(&data).enumerate() {
                fonts.push((
                    FontPath {
                        path: path.clone(),
                        index: index as u32,
                    },
                    info,
                ));
            }
        }
    }
}

/// Whether a path looks like a font file Typst can load (TrueType/OpenType,
/// including collections).
fn is_font_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ttf" | "otf" | "ttc" | "otc")
    )
}

fn build_packages(cache_dir: Option<PathBuf>) -> Option<SystemPackages> {
    let cache_dir = cache_dir?;
    let downloader = SystemDownloader::new("friday-typst");
    let universe = UniversePackages::new(downloader);
    let cache = FsPackages::new(cache_dir);
    Some(SystemPackages::from_parts(None, Some(cache), universe))
}

/// Normalizes a project-relative path into the `/leading/slash` key used by
/// Typst's `VirtualPath`.
fn virtual_key(path: &str) -> anyhow::Result<String> {
    let vpath = VirtualPath::new(path)
        .map_err(|err| anyhow::anyhow!("invalid project path {path:?}: {err:?}"))?;
    Ok(vpath.get_with_slash().to_string())
}

/// Current UTC date, or `None` if the system clock predates the Unix epoch.
fn system_today() -> Option<Datetime> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    Datetime::from_ymd(year, month, day)
}

/// Converts a count of days since the Unix epoch into a `(year, month, day)`
/// civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(main_src: &str, format: TypstFormat) -> CompileRequest {
        let mut files = BTreeMap::new();
        files.insert("main.typ".to_string(), main_src.as_bytes().to_vec());
        CompileRequest {
            files,
            main: "main.typ".to_string(),
            format,
            ..Default::default()
        }
    }

    #[test]
    fn compiles_pdf() {
        let out = compile(project("= Hello\nWorld", TypstFormat::Pdf)).unwrap();
        assert_eq!(&out[..5], b"%PDF-", "expected a PDF header");
    }

    #[test]
    fn compiles_svg() {
        let out = compile(project("Hello", TypstFormat::Svg)).unwrap();
        let svg = String::from_utf8(out).unwrap();
        assert!(svg.contains("<svg"), "expected svg markup, got: {svg:.60}");
    }

    #[test]
    fn compiles_png() {
        let out = compile(project("Hello", TypstFormat::Png)).unwrap();
        assert_eq!(
            &out[..8],
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
            "expected a PNG header"
        );
    }

    #[test]
    fn extreme_ppi_is_clamped() {
        let mut request = project("Hello", TypstFormat::Png);
        request.ppi = 1_000_000.0;
        let out = compile(request).unwrap();
        assert_eq!(&out[..8], &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
        // Clamped to MAX_PPI, a short line stays well under a gigapixel raster.
        assert!(
            out.len() < 50 * 1024 * 1024,
            "png unexpectedly large: {}",
            out.len()
        );
    }

    #[test]
    fn non_finite_ppi_falls_back() {
        let mut request = project("Hello", TypstFormat::Png);
        request.ppi = f32::NAN;
        let out = compile(request).unwrap();
        assert_eq!(&out[..4], &[0x89, 0x50, 0x4e, 0x47]);
    }

    #[test]
    fn resolves_project_import() {
        let mut files = BTreeMap::new();
        files.insert(
            "main.typ".to_string(),
            b"#import \"sub.typ\": msg\n#msg".to_vec(),
        );
        files.insert("sub.typ".to_string(), b"#let msg = [Hi]".to_vec());
        let request = CompileRequest {
            files,
            main: "main.typ".to_string(),
            ..Default::default()
        };
        let out = compile(request).unwrap();
        assert_eq!(&out[..5], b"%PDF-");
    }

    #[test]
    fn sys_inputs_are_exposed() {
        let mut request = project("#sys.inputs.at(\"name\")", TypstFormat::Pdf);
        request
            .sys_inputs
            .insert("name".to_string(), "Friday".to_string());
        let out = compile(request).unwrap();
        assert_eq!(&out[..5], b"%PDF-");
    }

    #[test]
    fn missing_main_is_rejected() {
        let request = CompileRequest {
            main: "absent.typ".to_string(),
            ..Default::default()
        };
        let err = compile(request).unwrap_err();
        assert!(err.to_string().contains("not present"), "{err}");
    }

    #[test]
    fn syntax_error_is_reported() {
        let err = compile(project("#let = 1", TypstFormat::Pdf)).unwrap_err();
        assert!(err.to_string().contains("error"), "{err}");
    }

    #[test]
    fn uncached_package_without_network_fails_clearly() {
        let request = CompileRequest {
            files: BTreeMap::from([(
                "main.typ".to_string(),
                b"#import \"@preview/example:0.1.0\": *".to_vec(),
            )]),
            main: "main.typ".to_string(),
            package_cache: Some(std::env::temp_dir().join("friday-typst-test-empty-cache")),
            allow_network: false,
            ..Default::default()
        };
        let err = compile(request).unwrap_err();
        assert!(
            err.to_string().contains("network access is disabled"),
            "{err}"
        );
    }

    #[test]
    fn format_parsing() {
        assert_eq!(TypstFormat::parse("PDF"), Some(TypstFormat::Pdf));
        assert_eq!(
            TypstFormat::from_output_path("a/b.svg"),
            Some(TypstFormat::Svg)
        );
        assert_eq!(TypstFormat::parse("docx"), None);
    }

    #[test]
    fn civil_date_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }
}
