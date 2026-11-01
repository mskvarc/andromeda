//! Response-compression policy: which codings, at what quality.

use crate::{
    comma_list::array_or_comma_list,
    schema::{content_coding::ContentCoding, feature_mode::FeatureMode},
};
use enumset::EnumSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How hard the response compressor works, trading CPU against ratio.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "How hard the response compressor works, trading CPU time against compression ratio.")]
pub enum CompressionQuality {
    /// Favour speed over compression ratio.
    #[schemars(description = "Favours speed over compression ratio.")]
    Fastest,
    /// Favour compression ratio over speed.
    #[schemars(description = "Favours compression ratio over speed.")]
    Best,
    /// The coding's own balanced default.
    #[default]
    #[schemars(description = "Uses each coding's own balance of speed and ratio.")]
    Default,
}

/// Response-compression configuration.
///
/// `mode` turns compression on or off; the codings are a typed set that must
/// name at least one coding while compression is on.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "Response compression: which codings, at what quality.")]
pub struct CompressionConfig {
    /// Whether responses are compressed at all.
    #[schemars(description = "Whether responses are compressed. Turn it off when a proxy in front compresses them.")]
    pub mode: FeatureMode,
    /// The content codings the broker may compress responses with.
    #[serde(deserialize_with = "array_or_comma_list")]
    #[schemars(
        with = "Vec<ContentCoding>",
        description = "The codings responses may be compressed with, chosen against the client's `Accept-Encoding`. Read only when `http.compression.mode` is `\"enabled\"`."
    )]
    pub algorithms: EnumSet<ContentCoding>,
    /// How hard the compressor works.
    #[schemars(description = "How hard the compressor works.")]
    pub quality: CompressionQuality,
}

impl Default for CompressionConfig {
    fn default() -> CompressionConfig {
        CompressionConfig {
            mode: FeatureMode::Enabled,
            algorithms: EnumSet::all(),
            quality: CompressionQuality::Default,
        }
    }
}

impl CompressionConfig {
    /// The codings responses are compressed with: none while compression is
    /// off, `algorithms` while it is on.
    #[must_use]
    pub const fn active_algorithms(&self) -> EnumSet<ContentCoding> {
        match self.mode {
            FeatureMode::Enabled => self.algorithms,
            FeatureMode::Disabled => EnumSet::empty(),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        content_coding::ContentCoding,
        feature_mode::FeatureMode,
        http::compression::{CompressionConfig, CompressionQuality},
    };
    use enumset::EnumSet;

    #[test]
    fn default_enables_every_coding() {
        pretty_assertions::assert_eq!(CompressionConfig::default().algorithms, EnumSet::<ContentCoding>::all());
    }

    #[test]
    fn compression_is_on_by_default_and_compresses_with_nothing_while_off() {
        let mut config = CompressionConfig::default();
        pretty_assertions::assert_eq!(config.mode, FeatureMode::Enabled);
        pretty_assertions::assert_eq!(config.active_algorithms(), EnumSet::<ContentCoding>::all());
        config.mode = FeatureMode::Disabled;
        assert!(config.active_algorithms().is_empty());
    }

    #[test]
    fn algorithms_array_parses_from_toml() {
        let config: CompressionConfig =
            toml::from_str("mode = \"enabled\"\nalgorithms = [\"gzip\", \"brotli\"]\nquality = \"best\"").expect("compression parses");
        pretty_assertions::assert_eq!(config.algorithms, ContentCoding::Gzip | ContentCoding::Brotli);
        pretty_assertions::assert_eq!(config.quality, CompressionQuality::Best);
    }

    #[test]
    fn compression_config_round_trips_through_toml() {
        let config = CompressionConfig {
            mode: FeatureMode::Disabled,
            algorithms: ContentCoding::Gzip | ContentCoding::Zstd,
            quality: CompressionQuality::Fastest,
        };
        let rendered = toml::to_string(&config).expect("compression serialises");
        let parsed: CompressionConfig = toml::from_str(&rendered).expect("compression parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
