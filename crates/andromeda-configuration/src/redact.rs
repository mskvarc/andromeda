//! The effective configuration with its secrets hidden, for operator output.

use crate::broker_config::BrokerConfig;
use serde_json::Error as JsonError;
use thiserror::Error;
use toml::ser::Error as TomlError;
use url::Url;
use url_macro::url;

/// What a database URL is replaced with: it names no host and carries no
/// credential.
fn redacted_database_url() -> Url {
    url!("postgres://redacted.invalid/redacted")
}

/// The effective configuration could not be written out.
#[derive(Debug, Error)]
pub enum RenderError {
    /// Writing JSON failed.
    #[error("failed to serialize effective configuration as JSON: {0}")]
    Json(#[from] JsonError),
    /// Writing TOML failed.
    #[error("failed to serialize effective configuration as TOML: {0}")]
    Toml(#[from] TomlError),
}

/// The configuration with both database URLs, the direct one and the pooler's,
/// replaced, since either may carry a password.
#[must_use]
pub fn redacted(config: &BrokerConfig) -> BrokerConfig {
    // clone: the output is a printable copy; the live configuration is not changed.
    let mut redacted = config.clone();
    redacted.database.url = redacted_database_url();
    if let Some(pooler) = &mut redacted.database.pooler {
        pooler.url = redacted_database_url();
    }
    redacted
}

/// The redacted configuration as pretty JSON.
///
/// # Errors
///
/// Returns [`RenderError::Json`] when writing fails.
pub fn redacted_json(config: &BrokerConfig) -> Result<String, RenderError> {
    Ok(serde_json::to_string_pretty(&redacted(config))?)
}

/// The redacted configuration as pretty TOML.
///
/// # Errors
///
/// Returns [`RenderError::Toml`] when writing fails.
pub fn redacted_toml(config: &BrokerConfig) -> Result<String, RenderError> {
    Ok(toml::to_string_pretty(&redacted(config))?)
}
