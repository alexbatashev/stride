//! URLs for built frontend assets.
//!
//! Argon assets use the generated content-hashed manifest. App-owned CSS and
//! widget assets keep stable names; shell CSS gets a startup content hash.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::OnceLock;

static VERSIONS: OnceLock<HashMap<&'static str, String>> = OnceLock::new();

/// Assets referenced by the server-rendered HTML shell. These are the URLs a
/// browser must refetch after a deploy for the app to update.
const VERSIONED_ASSETS: &[&str] = &["common.css"];

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
    if let Some(asset) = crate::components::argon_assets::asset(rel) {
        return format!("/static/{asset}");
    }
    match VERSIONS.get().and_then(|m| m.get(rel)) {
        Some(hash) => format!("/static/{rel}?v={hash}"),
        None => format!("/static/{rel}"),
    }
}

pub fn modulepreload_urls(rel: &str) -> Vec<String> {
    crate::components::argon_assets::modulepreload(rel)
        .iter()
        .map(|asset| format!("/static/{asset}"))
        .collect()
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
