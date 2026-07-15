//! Content-hash cache-busting for the built frontend bundles.
//!
//! The SSR shell references stable filenames such as `/static/components.js`.
//! Deploys keep those names, so browsers that cached them (see the `/static`
//! immutable policy) never refetch after an update. Nix pins store-path mtimes
//! to the epoch, so `Last-Modified` revalidation is useless as a fallback.
//!
//! We compute a short content hash per asset at startup and append it as a
//! `?v=` query. A changed bundle gets a new URL and is fetched fresh; an
//! unchanged one keeps its URL and stays cached. Query-only assets referenced
//! from generated widget HTML (`vendor/*`, `widget-frame.js`) keep stable URLs
//! and are intentionally excluded — see the `/static` cache middleware.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::OnceLock;

static VERSIONS: OnceLock<HashMap<&'static str, String>> = OnceLock::new();

/// Assets referenced by the server-rendered HTML shell. These are the URLs a
/// browser must refetch after a deploy for the app to update.
const VERSIONED_ASSETS: &[&str] = &[
    "common.css",
    "components.js",
    "api.js",
    "pages/threads-page.js",
    "pages/files-page.js",
    "pages/automations-page.js",
    "pages/archived-page.js",
];

/// Hash each shell asset's contents so `url` can append a cache-busting token.
/// Idempotent; safe to call once per process (from `app`).
pub fn init(static_dir: &Path) {
    let mut map = HashMap::new();
    for rel in VERSIONED_ASSETS {
        if let Ok(bytes) = std::fs::read(static_dir.join(rel)) {
            map.insert(*rel, content_hash(&bytes));
        }
    }
    let _ = VERSIONS.set(map);
}

fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// `/static/<rel>` with a `?v=<hash>` token when the asset was hashed at
/// startup, otherwise the plain path (dev/tests where `init` did not run).
pub fn url(rel: &str) -> String {
    match VERSIONS.get().and_then(|m| m.get(rel)) {
        Some(hash) => format!("/static/{rel}?v={hash}"),
        None => format!("/static/{rel}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash_is_deterministic_and_content_sensitive() {
        assert_eq!(super::content_hash(b"same"), super::content_hash(b"same"));
        assert_ne!(super::content_hash(b"a"), super::content_hash(b"b"));
    }

    #[test]
    fn url_without_init_is_the_plain_path() {
        // `init` is process-global; this asset name is never registered.
        assert_eq!(
            super::url("never-registered.js"),
            "/static/never-registered.js"
        );
    }
}
