//! Cached JSON documents mutated only at explicit JSON Pointer locations.

use dashmap::DashMap;
use serde_json::Value;
use std::{fs, io, path::PathBuf, sync::OnceLock};
use thiserror::Error;

/// A validated RFC 6901 JSON Pointer used to mutate fixture fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonPointer {
    value: String,
}

impl JsonPointer {
    /// Validate an empty root pointer or a slash-prefixed JSON Pointer.
    ///
    /// # Errors
    ///
    /// Returns [`JsonPointerError`] when the value is neither empty nor prefixed with `/`,
    /// the two forms RFC 6901 permits.
    pub fn parse(value: impl Into<String>) -> Result<JsonPointer, JsonPointerError> {
        let value = value.into();
        if value.is_empty() || value.starts_with('/') {
            Ok(JsonPointer { value })
        } else {
            Err(JsonPointerError)
        }
    }

    /// Borrow the encoded pointer.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

/// Parsed fixture document owned by one test scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct FixtureDocument {
    value: Value,
}

impl FixtureDocument {
    /// Parse a fixture from JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError::Parse`] when the bytes are not a well-formed JSON document.
    pub fn parse(bytes: &[u8]) -> Result<FixtureDocument, FixtureError> {
        let value = serde_json::from_slice(bytes).map_err(FixtureError::Parse)?;
        Ok(FixtureDocument { value })
    }

    /// Load and cache a JSON fixture file, returning an independently mutable document.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError::Read`] when the file cannot be read from disk, and
    /// [`FixtureError::Parse`] when its contents are not well-formed JSON.
    pub fn load(path: PathBuf) -> Result<FixtureDocument, FixtureError> {
        let cache = fixture_cache();
        if let Some(value) = cache.get(&path) {
            // Each test must own its document because pointer replacement mutates it.
            return Ok(FixtureDocument { value: value.clone() }); // clone: each test receives an independently mutable cached document.
        }
        let bytes = fs::read(&path).map_err(|source| FixtureError::Read { path: path.clone(), source })?; // clone: an error owns the path while the success path still inserts it into the cache.
        let document = FixtureDocument::parse(&bytes)?;
        // The cache and returned document need independent ownership across parallel tests.
        cache.insert(path, document.value.clone()); // clone: the cache and returned document need independent ownership.
        Ok(document)
    }

    /// Replace the value at an existing pointer without textual substitution.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError::PointerNotFound`] when the document holds no value at the
    /// pointer, in which case the document is left unchanged.
    pub fn replace(&mut self, pointer: &JsonPointer, replacement: Value) -> Result<(), FixtureError> {
        let target = self
            .value
            .pointer_mut(pointer.as_str())
            .ok_or_else(|| FixtureError::PointerNotFound(pointer.clone()))?; // clone: the error owns the pointer borrowed from the caller.
        *target = replacement;
        Ok(())
    }

    /// Consume the document and return its JSON value.
    #[must_use]
    pub fn into_value(self) -> Value {
        self.value
    }
}

type FixtureCache = DashMap<PathBuf, Value>;

fn fixture_cache() -> &'static FixtureCache {
    static CACHE: OnceLock<FixtureCache> = OnceLock::new();
    CACHE.get_or_init(FixtureCache::new)
}

/// Invalid JSON Pointer syntax.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("JSON Pointer must be empty or begin with '/'")]
pub struct JsonPointerError;

/// Fixture loading, parsing, or mutation failure.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// The fixture could not be read.
    #[error("failed to read fixture {}: {source}", path.display())]
    Read {
        /// Filesystem path of the fixture.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: io::Error,
    },
    /// The fixture does not contain valid JSON.
    #[error("fixture is not valid JSON: {0}")]
    Parse(#[source] serde_json::Error),
    /// The requested mutation pointer is absent.
    #[error("fixture has no value at pointer {}", .0.as_str())]
    PointerNotFound(JsonPointer),
}

#[cfg(test)]
mod tests {
    use crate::fixture_document::{FixtureDocument, FixtureError, JsonPointer};
    use serde_json::{Value, json};

    #[test]
    fn replacement_preserves_string_escaping_and_json_types() {
        let mut document = FixtureDocument::parse(br#"{"text":"old","nested":null}"#).expect("static fixture is valid JSON");
        let text = JsonPointer::parse("/text").expect("static pointer is valid");
        let nested = JsonPointer::parse("/nested").expect("static pointer is valid");

        document
            .replace(&text, Value::String("a \"quoted\" value".to_owned()))
            .expect("the text pointer exists in the fixture");
        document
            .replace(&nested, json!({"items": [1, 2]}))
            .expect("the nested pointer exists in the fixture");
        let value = document.into_value();

        assert_eq!(value["text"], "a \"quoted\" value");
        assert_eq!(value["nested"], json!({"items": [1, 2]}));
    }

    #[test]
    fn missing_pointer_returns_a_typed_error() {
        let mut document = FixtureDocument::parse(br#"{"present":1}"#).expect("static fixture is valid JSON");
        let pointer = JsonPointer::parse("/missing").expect("static pointer is valid");
        let result = document.replace(&pointer, Value::Null);

        assert!(matches!(result, Err(FixtureError::PointerNotFound(actual)) if actual == pointer));
    }
}
