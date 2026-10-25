//! Response-compression policy: which codings, at what quality.

use crate::config::content_coding::ContentCoding;
use enumset::EnumSet;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use toml_comment::TomlComment;

/// How hard the response compressor works, trading CPU against ratio.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum CompressionQuality {
    /// Favour speed over compression ratio.
    Fastest,
    /// Favour compression ratio over speed.
    Best,
    /// The coding's own balanced default.
    #[default]
    Default,
}

/// Response-compression configuration.
///
/// The enabled codings are a single typed set listed as an array; an empty set
/// means response compression is off, so there is no separate umbrella toggle.
#[derive(Debug, Clone, Deserialize, Serialize, TomlComment, PartialEq, Eq)]
pub struct CompressionConfig {
    /// The content codings the broker may compress responses with. An empty
    /// array disables response compression entirely.
    pub algorithms: EnumSet<ContentCoding>,
    /// How hard the compressor works.
    #[toml_comment(inline)]
    pub quality: CompressionQuality,
}

impl Default for CompressionConfig {
    fn default() -> CompressionConfig {
        CompressionConfig {
            algorithms: EnumSet::all(),
            quality: CompressionQuality::Default,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::{
        compression::{CompressionConfig, CompressionQuality},
        content_coding::ContentCoding,
    };
    use enumset::EnumSet;

    #[test]
    fn default_enables_every_coding() {
        pretty_assertions::assert_eq!(CompressionConfig::default().algorithms, EnumSet::<ContentCoding>::all());
    }

    #[test]
    fn algorithms_array_parses_from_toml() {
        let config: CompressionConfig = toml::from_str("algorithms = [\"gzip\", \"brotli\"]\nquality = \"best\"").expect("compression parses");
        pretty_assertions::assert_eq!(config.algorithms, ContentCoding::Gzip | ContentCoding::Brotli);
        pretty_assertions::assert_eq!(config.quality, CompressionQuality::Best);
    }

    #[test]
    fn compression_config_round_trips_through_toml() {
        let config = CompressionConfig {
            algorithms: ContentCoding::Gzip | ContentCoding::Zstd,
            quality: CompressionQuality::Fastest,
        };
        let rendered = toml::to_string(&config).expect("compression serialises");
        let parsed: CompressionConfig = toml::from_str(&rendered).expect("compression parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
