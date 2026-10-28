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

/// The dotted paths of the keys [`redacted`] replaces. The configuration
/// descriptor marks exactly these sensitive.
pub const SENSITIVE_KEYS: [&str; 2] = ["database.url", "database.pooler.url"];

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

#[cfg(test)]
mod tests {
    use super::{SENSITIVE_KEYS, redacted};
    use crate::{broker_config::BrokerConfig, schema::database::pooler::PoolerConfig};
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use url_macro::url;

    /// The dotted path of every leaf of `value` with its value.
    fn leaves(value: &Value, path: &str, into: &mut Vec<(String, Value)>) {
        match value {
            Value::Object(members) => members
                .iter()
                .for_each(|(name, member)| leaves(member, &if path.is_empty() { name.clone() } else { format!("{path}.{name}") }, into)),
            leaf @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => into.push((path.to_owned(), leaf.clone())),
        }
    }

    #[test]
    fn redaction_changes_exactly_the_sensitive_keys() -> Result<(), serde_json::Error> {
        let mut config = BrokerConfig::default();
        config.database.url = url!("postgres://user:secret@db.example/andromeda");
        config.database.pooler = Some(PoolerConfig {
            url: url!("postgres://user:secret@pooler.example/andromeda"),
            ..PoolerConfig::default()
        });
        let (mut before, mut after) = (Vec::new(), Vec::new());
        leaves(&serde_json::to_value(&config)?, "", &mut before);
        leaves(&serde_json::to_value(redacted(&config))?, "", &mut after);
        let changed: BTreeSet<&str> = before
            .iter()
            .zip(&after)
            .filter(|(before, after)| before != after)
            .map(|(before, _)| before.0.as_str())
            .collect();
        assert_eq!(changed, SENSITIVE_KEYS.into_iter().collect::<BTreeSet<_>>());
        Ok(())
    }
}
