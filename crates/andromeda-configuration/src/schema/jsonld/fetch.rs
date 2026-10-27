//! The `[jsonld.fetch]` table: whether and how a remote `@context` is
//! downloaded.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration, schema::feature_mode::FeatureMode};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use toml_comment::TomlComment;

/// An unusable `[jsonld.fetch]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JsonLdFetchConfigError {
    /// `timeout` was below one second, which would abandon nearly every fetch
    /// before a remote origin server could answer.
    #[error("jsonld.fetch.timeout must be at least 1 second")]
    TimeoutBelowOneSecond,
    /// `max_size` was zero, which would refuse every context.
    #[error("jsonld.fetch.max_size must be at least 1 byte")]
    ZeroMaxSize,
}

/// Whether and how a remote `@context` is downloaded.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JsonLdFetchConfig {
    /// Whether a context that is neither embedded, preloaded nor cached may
    /// be downloaded: `enabled` or `disabled`.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
    /// How long one download may take, in human-readable form (`"30s"`); at
    /// least one second.
    #[toml_comment(inline)]
    pub timeout: HumanDuration,
    /// How many redirects one download may follow.
    #[toml_comment(inline)]
    pub max_redirects: u32,
    /// The size beyond which a downloaded context is refused, in
    /// human-readable form (`"2 MiB"`).
    #[toml_comment(inline)]
    pub max_size: HumanByteSize,
}

impl Default for JsonLdFetchConfig {
    fn default() -> JsonLdFetchConfig {
        JsonLdFetchConfig {
            mode: FeatureMode::Enabled,
            timeout: HumanDuration::from_secs(30),
            max_redirects: 5,
            max_size: HumanByteSize::mib(2),
        }
    }
}

impl JsonLdFetchConfig {
    /// Validate the download limits.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdFetchConfigError::TimeoutBelowOneSecond`] or
    /// [`JsonLdFetchConfigError::ZeroMaxSize`].
    pub fn validate(&self) -> Result<(), JsonLdFetchConfigError> {
        if self.timeout.duration() < Duration::from_secs(1) {
            return Err(JsonLdFetchConfigError::TimeoutBelowOneSecond);
        }
        if self.max_size.as_u64() == 0 {
            return Err(JsonLdFetchConfigError::ZeroMaxSize);
        }
        Ok(())
    }

    /// Whether a remote context may be downloaded.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self.mode, FeatureMode::Enabled)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        schema::jsonld::fetch::{JsonLdFetchConfig, JsonLdFetchConfigError},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_downloads_up_to_two_mebibytes_within_thirty_seconds() {
        let config = JsonLdFetchConfig::default();
        assert!(config.is_enabled());
        assert_eq!(config.max_size, HumanByteSize::mib(2));
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn a_sub_second_timeout_and_a_zero_size_are_refused() {
        let slow = JsonLdFetchConfig {
            timeout: HumanDuration::from_millis(500),
            ..JsonLdFetchConfig::default()
        };
        assert_eq!(slow.validate(), Err(JsonLdFetchConfigError::TimeoutBelowOneSecond));
        let empty = JsonLdFetchConfig {
            max_size: HumanByteSize::b(0),
            ..JsonLdFetchConfig::default()
        };
        assert_eq!(empty.validate(), Err(JsonLdFetchConfigError::ZeroMaxSize));
    }
}
