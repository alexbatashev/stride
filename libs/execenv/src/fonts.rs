//! Mountable font cache shared by the Python sandbox and the Typst compiler.
//!
//! On first run a small set of popular open-source font families is downloaded
//! into `<cache_dir>/fonts` and never re-fetched (a per-family marker records the
//! source URL). The directory is mounted read-only into the eryx sandbox at
//! [`crate::FONTS_GUEST_DIR`] so matplotlib, fpdf2, Pillow and friends find real
//! fonts instead of failing on missing system paths, and it is also scanned by
//! the host-side Typst compiler.
//!
//! DejaVu ships as a plain zip from a stable GitHub release. The Google Fonts
//! families are fetched through the `download/list` JSON manifest endpoint: the
//! old `download?family=` zip endpoint now answers with an HTML catalog page
//! (so unzipping it failed with "Could not find EOCD"), whereas the manifest
//! lists every file with a direct `fonts.gstatic.com` URL we download into the
//! per-family directory.
//!
//! Downloads are best-effort: a family that fails to fetch is logged and skipped
//! so a flaky font mirror never breaks Python execution. DejaVu (matplotlib's
//! default family) ships from a stable GitHub release, so the common case keeps
//! working even when the Google Fonts mirror is unreachable.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;

use crate::{ArtifactKind, ArtifactSpec, ArtifactStore};

/// Where a font family's files come from.
enum FontSource {
    Pinned(&'static ArtifactSpec),
    /// A Google Fonts `download/list` JSON manifest; font files are pulled
    /// individually from the `fonts.gstatic.com` URLs it lists.
    GoogleManifest,
}

/// One font family to install, unpacked into a per-family subdirectory; nested
/// directories are fine because every consumer scans recursively.
struct FontPackage {
    /// Stable key used for the per-family install marker.
    name: &'static str,
    url: &'static str,
    source: FontSource,
}

// DejaVu ships from a versioned GitHub release. It is matplotlib's default
// family and covers a wide Unicode range, so it is the reliable baseline that
// keeps plotting working even if the Google Fonts families below fail to fetch.
const DEJAVU_URL: &str = "https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.zip";
const DEJAVU: ArtifactSpec = ArtifactSpec {
    name: "dejavu",
    url: DEJAVU_URL,
    sha256: "7576310b219e04159d35ff61dd4a4ec4cdba4f35c00e002a136f00e96a908b0a",
    kind: ArtifactKind::Zip,
};

// Google Fonts manifests and their referenced files have no stable upstream
// hashes. They are the documented unpinned exception and retain URL-only markers.
// Spaces are percent-encoded. Roboto, Open Sans, Lato and Montserrat are popular
// Latin UI families; the Noto trio adds broad script and monospace coverage.
const ROBOTO_URL: &str = "https://fonts.google.com/download/list?family=Roboto";
const OPEN_SANS_URL: &str = "https://fonts.google.com/download/list?family=Open%20Sans";
const LATO_URL: &str = "https://fonts.google.com/download/list?family=Lato";
const MONTSERRAT_URL: &str = "https://fonts.google.com/download/list?family=Montserrat";
const NOTO_SANS_URL: &str = "https://fonts.google.com/download/list?family=Noto%20Sans";
const NOTO_SERIF_URL: &str = "https://fonts.google.com/download/list?family=Noto%20Serif";
const NOTO_SANS_MONO_URL: &str = "https://fonts.google.com/download/list?family=Noto%20Sans%20Mono";

const FONT_PACKAGES: &[FontPackage] = &[
    FontPackage {
        name: "dejavu",
        url: DEJAVU_URL,
        source: FontSource::Pinned(&DEJAVU),
    },
    FontPackage {
        name: "roboto",
        url: ROBOTO_URL,
        source: FontSource::GoogleManifest,
    },
    FontPackage {
        name: "open-sans",
        url: OPEN_SANS_URL,
        source: FontSource::GoogleManifest,
    },
    FontPackage {
        name: "lato",
        url: LATO_URL,
        source: FontSource::GoogleManifest,
    },
    FontPackage {
        name: "montserrat",
        url: MONTSERRAT_URL,
        source: FontSource::GoogleManifest,
    },
    FontPackage {
        name: "noto-sans",
        url: NOTO_SANS_URL,
        source: FontSource::GoogleManifest,
    },
    FontPackage {
        name: "noto-serif",
        url: NOTO_SERIF_URL,
        source: FontSource::GoogleManifest,
    },
    FontPackage {
        name: "noto-sans-mono",
        url: NOTO_SANS_MONO_URL,
        source: FontSource::GoogleManifest,
    },
];

/// A Google Fonts `download/list` response: text files are inlined, binary font
/// files are referenced by URL. Only the pieces we consume are modelled.
#[derive(serde::Deserialize)]
struct GoogleFontManifest {
    manifest: GoogleFontFiles,
}

#[derive(serde::Deserialize)]
struct GoogleFontFiles {
    #[serde(default, rename = "fileRefs")]
    file_refs: Vec<GoogleFontFileRef>,
}

#[derive(serde::Deserialize)]
struct GoogleFontFileRef {
    filename: String,
    url: String,
}

impl GoogleFontManifest {
    /// Font files worth installing. Variable fonts and top-level weights cover a
    /// family in a handful of files, so the redundant `static/` directory (often
    /// dozens of per-weight TTFs) is skipped; families that ship only static
    /// weights fall back to downloading everything.
    fn font_files(&self) -> Vec<&GoogleFontFileRef> {
        let is_font = |f: &&GoogleFontFileRef| {
            let name = f.filename.to_ascii_lowercase();
            name.ends_with(".ttf") || name.ends_with(".otf")
        };
        let top: Vec<&GoogleFontFileRef> = self
            .manifest
            .file_refs
            .iter()
            .filter(|f| !f.filename.starts_with("static/"))
            .filter(is_font)
            .collect();
        if !top.is_empty() {
            return top;
        }
        self.manifest.file_refs.iter().filter(is_font).collect()
    }
}

/// Ensures the font cache exists, downloading any missing families, and returns
/// the directory. A family that fails to download is logged and skipped so the
/// sandbox still starts with whatever fonts are already present.
pub async fn ensure_fonts(cache_dir: &Path) -> anyhow::Result<PathBuf> {
    let fonts_dir = cache_dir.join("fonts");
    let markers = fonts_dir.join(".installed");
    tokio::fs::create_dir_all(&markers).await?;

    for pkg in FONT_PACKAGES {
        if let Err(err) = install_font(pkg, &fonts_dir, &markers).await {
            tracing::warn!(family = pkg.name, %err, "failed to install font family");
        }
    }

    Ok(fonts_dir)
}

async fn install_font(pkg: &FontPackage, fonts_dir: &Path, markers: &Path) -> anyhow::Result<()> {
    if let FontSource::Pinned(spec) = pkg.source {
        return ArtifactStore::new(fonts_dir)
            .ensure_best_effort(spec)
            .await
            .map(|_| ())
            .ok_or_else(|| anyhow::anyhow!("font artifact is unavailable"));
    }

    let marker = markers.join(pkg.name);
    if tokio::fs::read_to_string(&marker).await.ok().as_deref() == Some(pkg.url) {
        return Ok(());
    }

    let target = fonts_dir.join(pkg.name);
    let staging = fonts_dir.join(".downloads").join(pkg.name);
    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::create_dir_all(&staging).await?;

    match pkg.source {
        FontSource::Pinned(_) => unreachable!(),
        FontSource::GoogleManifest => install_google_family(pkg, fonts_dir, &staging).await?,
    }

    let _ = tokio::fs::remove_dir_all(&target).await;
    tokio::fs::rename(staging, target).await?;
    tokio::fs::write(&marker, pkg.url).await?;
    Ok(())
}

async fn install_google_family(
    pkg: &FontPackage,
    fonts_dir: &Path,
    target: &Path,
) -> anyhow::Result<()> {
    // Drop the broken HTML "zip" earlier builds saved here before this endpoint
    // switched to JSON, so the cache is left clean.
    let _ = tokio::fs::remove_file(fonts_dir.join(format!("{}.zip", pkg.name))).await;

    let raw = tokio::time::timeout(Duration::from_secs(60), crate::artifacts::fetch(pkg.url))
        .await
        .context("google fonts manifest fetch timed out")??;
    let manifest: GoogleFontManifest = serde_json::from_slice(strip_xssi_prefix(raw.as_ref()))
        .context("parse google fonts manifest")?;

    let files = manifest.font_files();
    anyhow::ensure!(!files.is_empty(), "manifest listed no font files");
    for file in files {
        let dest = safe_join(target, &file.filename)?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        crate::artifacts::download(&file.url, &dest).await?;
    }
    Ok(())
}

/// Strips Google's anti-JSON-hijacking `)]}'` prefix line, if present.
fn strip_xssi_prefix(body: &[u8]) -> &[u8] {
    const PREFIX: &[u8] = b")]}'";
    match body.strip_prefix(PREFIX) {
        Some(rest) => match rest.iter().position(|&b| b == b'\n') {
            Some(nl) => &rest[nl + 1..],
            None => rest,
        },
        None => body,
    }
}

/// Joins a manifest-supplied relative path onto the family directory, rejecting
/// absolute paths and any `..`/root components so a hostile manifest cannot
/// escape the cache.
fn safe_join(base: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = Path::new(rel);
    anyhow::ensure!(rel.is_relative(), "font filename not relative: {rel:?}");
    for comp in rel.components() {
        anyhow::ensure!(
            matches!(comp, Component::Normal(_)),
            "unsafe font filename: {rel:?}"
        );
    }
    Ok(base.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_manifest_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        for pkg in FONT_PACKAGES {
            assert!(names.insert(pkg.name), "duplicate font name {}", pkg.name);
            assert!(
                pkg.url.starts_with("https://"),
                "{} url not https",
                pkg.name
            );
        }
    }

    #[test]
    fn dejavu_is_present_as_the_baseline_family() {
        assert!(
            FONT_PACKAGES
                .iter()
                .any(|pkg| pkg.name == "dejavu" && matches!(pkg.source, FontSource::Pinned(_))),
            "DejaVu must ship as the matplotlib default fallback"
        );
    }

    #[test]
    fn strip_xssi_prefix_drops_guard_line() {
        assert_eq!(strip_xssi_prefix(b")]}'\n{\"a\":1}"), b"{\"a\":1}");
        assert_eq!(strip_xssi_prefix(b"{\"a\":1}"), b"{\"a\":1}");
    }

    #[test]
    fn font_files_prefers_variable_over_static() {
        let json = br#")]}'
{"zipName":"Roboto.zip","manifest":{"files":[{"filename":"OFL.txt","contents":"x"}],
"fileRefs":[
{"filename":"Roboto-VariableFont_wght.ttf","url":"https://fonts.gstatic.com/a.ttf"},
{"filename":"static/Roboto-Thin.ttf","url":"https://fonts.gstatic.com/b.ttf"},
{"filename":"README.txt","url":"https://fonts.gstatic.com/r.txt"}]}}"#;
        let m: GoogleFontManifest = serde_json::from_slice(strip_xssi_prefix(json)).unwrap();
        let files = m.font_files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "Roboto-VariableFont_wght.ttf");
    }

    #[test]
    fn font_files_falls_back_to_static_only_families() {
        let json = br#"{"manifest":{"fileRefs":[
{"filename":"static/Lato-Regular.ttf","url":"https://fonts.gstatic.com/l.ttf"}]}}"#;
        let m: GoogleFontManifest = serde_json::from_slice(json).unwrap();
        assert_eq!(m.font_files().len(), 1);
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let base = Path::new("/cache/fonts/roboto");
        assert!(safe_join(base, "static/Roboto.ttf").is_ok());
        assert!(safe_join(base, "../escape.ttf").is_err());
        assert!(safe_join(base, "/etc/passwd").is_err());
    }
}
