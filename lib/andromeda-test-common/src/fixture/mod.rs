//! JSON-fixture loader for integration tests.
//!
//! Loads canonical payloads placed on disk under
//! `lib/andromeda-test-common/fixtures/`: it resolves paths relative to the
//! crate's `CARGO_MANIFEST_DIR`, reads the file once per process, applies
//! placeholder substitution, and returns a fresh [`Value`] per call so tests
//! can mutate the result without contaminating the cache. The `fixtures/` tree
//! is available but currently unpopulated — suites still inline their payloads —
//! so the loader is here for suites that want to move payloads out of Rust
//! source, not a directory every suite already reads from.
//!
//! Placeholder substitution uses `{{identifier}}` tokens. The loader accepts a
//! [`Bindings`] table — a flat key-value list — and replaces every occurrence
//! of `{{key}}` in the raw JSON text with the binding's value.

pub mod csr;
pub mod entity;
pub mod snapshot;
pub mod subscription;
pub mod temporal;

use dashmap::DashMap;
use serde_json::Value;
use std::{path::PathBuf, sync::OnceLock};
use thiserror::Error;

/// Error type for fixture loading.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// Filesystem read failed.
    #[error("io error reading fixture {path}: {source}")]
    Io {
        /// Fixture path relative to `fixtures/`.
        path: String,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// JSON parse failed after placeholder substitution.
    #[error("json parse error in fixture {path}: {source}")]
    Parse {
        /// Fixture path relative to `fixtures/`.
        path: String,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
}

type FixtureCache = DashMap<String, String>;

fn cache() -> &'static FixtureCache {
    static CACHE: OnceLock<FixtureCache> = OnceLock::new();
    CACHE.get_or_init(FixtureCache::new)
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Key-value bindings substituted into `{{key}}` placeholders.
pub type Bindings<'a> = &'a [(&'a str, &'a str)];

/// Load `fixture_path` (e.g. `"entity/building_minimal.json"`) and return the
/// parsed JSON value.
pub fn load(fixture_path: &str) -> Result<Value, FixtureError> {
    load_with(fixture_path, &[])
}

/// Load `fixture_path` and substitute every `{{key}}` placeholder using
/// `bindings` before parsing.
///
/// Each `{{key}}` is replaced with its binding's value verbatim, so a binding
/// supplies whatever JSON text belongs at that position — a quoted string, a
/// number, or a nested array. The value is not quoted or otherwise escaped by
/// the loader.
pub fn load_with(fixture_path: &str, bindings: Bindings<'_>) -> Result<Value, FixtureError> {
    let raw = read_cached(fixture_path)?;
    let substituted = substitute(&raw, bindings);
    serde_json::from_str(&substituted).map_err(|source| FixtureError::Parse {
        path: fixture_path.to_owned(),
        source,
    })
}

fn read_cached(fixture_path: &str) -> Result<String, FixtureError> {
    let cache = cache();
    if let Some(cached) = cache.get(fixture_path) {
        return Ok(cached.clone());
    }
    let full_path = fixtures_root().join(fixture_path);
    let raw = std::fs::read_to_string(&full_path).map_err(|source| FixtureError::Io {
        path: fixture_path.to_owned(),
        source,
    })?;
    cache.insert(fixture_path.to_owned(), raw.clone());
    Ok(raw)
}

fn substitute(raw: &str, bindings: Bindings<'_>) -> String {
    let mut out = raw.to_owned();
    for (key, value) in bindings {
        let token = format!("{{{{{key}}}}}");
        out = out.replace(&token, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{load_with, substitute};

    #[test]
    fn substitute_replaces_every_occurrence() {
        let raw = "id = {{id}}; other id = {{id}}";
        let out = substitute(raw, &[("id", "abc")]);
        pretty_assertions::assert_eq!(out, "id = abc; other id = abc");
    }

    #[test]
    fn load_missing_fixture_returns_io_error() {
        let err = load_with("does/not/exist.json", &[]).unwrap_err();
        assert!(matches!(err, super::FixtureError::Io { .. }));
    }
}
