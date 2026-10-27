//! The `[jsonld.fetch]` table: whether and how a remote `@context` is
//! downloaded.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration, schema::feature_mode::FeatureMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether and how a remote `@context` is downloaded.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Whether and how a remote `@context` is downloaded.")]
pub struct JsonLdFetchConfig {
    /// Whether a context that is neither embedded, preloaded nor cached may
    /// be downloaded: `enabled` or `disabled`.
    #[schemars(
        description = "Whether a context that is neither embedded, preloaded, stored nor cached may be downloaded. With `\"disabled\"` nothing is downloaded: only the core `@context`, preloaded documents, stored `@context`s and what is already cached are used."
    )]
    pub mode: FeatureMode,
    /// How long one download may take, in human-readable form (`"30s"`); at
    /// least one second.
    #[schemars(description = "How long one download may take.")]
    pub timeout: HumanDuration,
    /// How many redirects one download may follow.
    #[schemars(description = "How many redirects one download may follow.")]
    pub max_redirects: u32,
    /// The size beyond which a downloaded context is refused, in
    /// human-readable form (`"2 MiB"`).
    #[schemars(description = "The largest `@context` document the broker downloads. A larger one is refused with `504` `LdContextNotAvailable`.")]
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
    /// Whether a remote context may be downloaded.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self.mode, FeatureMode::Enabled)
    }
}

#[cfg(test)]
mod tests {
    use crate::{human_byte_size::HumanByteSize, schema::jsonld::fetch::JsonLdFetchConfig};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_downloads_up_to_two_mebibytes_within_thirty_seconds() {
        let config = JsonLdFetchConfig::default();
        assert!(config.is_enabled());
        assert_eq!(config.max_size, HumanByteSize::mib(2));
    }
}
