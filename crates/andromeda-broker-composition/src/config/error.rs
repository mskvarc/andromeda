//! Focused failures produced while loading, validating, or rendering configuration.

use figment2::Error as FigmentError;
use serde_json::Error as JsonError;
use std::path::PathBuf;
use thiserror::Error;
use toml::ser::Error as TomlError;
use url::ParseError;

/// Failure owned by the broker configuration boundary.
#[derive(Debug, Error)]
pub(crate) enum ConfigurationError {
    /// Layered configuration loading failed.
    ///
    /// The figment error is boxed: it is by far the largest variant, and an
    /// unboxed copy would make every `Result<_, ConfigurationError>` in the
    /// loader carry its full width.
    #[error("loading configuration failed: {0}")]
    Load(#[source] Box<FigmentError>),
    /// An explicitly requested configuration file does not exist.
    ///
    /// A `--config` path names a file the operator expects to be read, so a
    /// missing one is a hard error; the default configuration file, by
    /// contrast, is loaded only when present.
    #[error("configuration file not found: {0}")]
    MissingFile(PathBuf),
    /// One configured value violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Invalid(String),
    /// The effective configuration could not be rendered as JSON.
    #[error("failed to serialize effective configuration as JSON: {0}")]
    Json(#[from] JsonError),
    /// The effective configuration could not be rendered as TOML.
    #[error("failed to serialize effective configuration as TOML: {0}")]
    Toml(#[from] TomlError),
    /// The compile-time redaction URL is invalid.
    #[error("invalid built-in redacted database URL: {0}")]
    RedactionUrl(#[from] ParseError),
}

impl ConfigurationError {
    /// Construct an invariant failure with operator-facing detail.
    pub(crate) fn invalid(detail: impl Into<String>) -> ConfigurationError {
        ConfigurationError::Invalid(detail.into())
    }
}
