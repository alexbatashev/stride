//! Mountable font cache shared by the Python sandbox and the Typst compiler.
//!
//! On first run a small set of popular open-source font families is downloaded
//! into `<cache_dir>/fonts` and never re-fetched (a per-family marker records the
//! source URL). The directory is mounted read-only into the eryx sandbox at
//! [`crate::FONTS_GUEST_DIR`] so matplotlib, fpdf2, Pillow and friends find real
//! fonts instead of failing on missing system paths, and it is also scanned by
//! the host-side Typst compiler.
//!
//! Downloads are best-effort: a family that fails to fetch is logged and skipped
//! so a flaky font mirror never breaks Python execution. DejaVu (matplotlib's
//! default family) ships from a stable GitHub release, so the common case keeps
//! working even when the Google Fonts mirror is unreachable.

use std::path::{Path, PathBuf};

/// One font family to install. Every source is a plain zip (Google Fonts
/// `download?family=` responses and the DejaVu GitHub release), unpacked into a
/// per-family subdirectory; nested directories are fine because every consumer
/// scans recursively.
struct FontPackage {
    /// Stable key used for the per-family install marker.
    name: &'static str,
    url: &'static str,
}

// DejaVu ships from a versioned GitHub release. It is matplotlib's default
// family and covers a wide Unicode range, so it is the reliable baseline that
// keeps plotting working even if the Google Fonts families below fail to fetch.
const DEJAVU_URL: &str = "https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.zip";

// Google Fonts families served as TTF zips by the `download?family=` endpoint.
// Spaces are percent-encoded. Roboto, Open Sans, Lato and Montserrat are popular
// Latin UI families; the Noto trio adds broad script and monospace coverage.
const ROBOTO_URL: &str = "https://fonts.google.com/download?family=Roboto";
const OPEN_SANS_URL: &str = "https://fonts.google.com/download?family=Open%20Sans";
const LATO_URL: &str = "https://fonts.google.com/download?family=Lato";
const MONTSERRAT_URL: &str = "https://fonts.google.com/download?family=Montserrat";
const NOTO_SANS_URL: &str = "https://fonts.google.com/download?family=Noto%20Sans";
const NOTO_SERIF_URL: &str = "https://fonts.google.com/download?family=Noto%20Serif";
const NOTO_SANS_MONO_URL: &str = "https://fonts.google.com/download?family=Noto%20Sans%20Mono";

const FONT_PACKAGES: &[FontPackage] = &[
    FontPackage {
        name: "dejavu",
        url: DEJAVU_URL,
    },
    FontPackage {
        name: "roboto",
        url: ROBOTO_URL,
    },
    FontPackage {
        name: "open-sans",
        url: OPEN_SANS_URL,
    },
    FontPackage {
        name: "lato",
        url: LATO_URL,
    },
    FontPackage {
        name: "montserrat",
        url: MONTSERRAT_URL,
    },
    FontPackage {
        name: "noto-sans",
        url: NOTO_SANS_URL,
    },
    FontPackage {
        name: "noto-serif",
        url: NOTO_SERIF_URL,
    },
    FontPackage {
        name: "noto-sans-mono",
        url: NOTO_SANS_MONO_URL,
    },
];

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
    let marker = markers.join(pkg.name);
    if tokio::fs::read_to_string(&marker).await.ok().as_deref() == Some(pkg.url) {
        return Ok(());
    }

    let target = fonts_dir.join(pkg.name);
    let _ = tokio::fs::remove_dir_all(&target).await;
    tokio::fs::create_dir_all(&target).await?;

    let archive = fonts_dir.join(format!("{}.zip", pkg.name));
    let _ = tokio::fs::remove_file(&archive).await;
    crate::download(pkg.url, &archive).await?;
    crate::extract_zip(&archive, &target).await?;
    let _ = tokio::fs::remove_file(&archive).await;
    tokio::fs::write(&marker, pkg.url).await?;
    Ok(())
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
            FONT_PACKAGES.iter().any(|pkg| pkg.name == "dejavu"),
            "DejaVu must ship as the matplotlib default fallback"
        );
    }
}
