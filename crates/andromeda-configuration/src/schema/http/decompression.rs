//! Request-decompression policy: which codings, and how to treat unaccepted ones.

use crate::{comma_list::array_or_comma_list, schema::content_coding::ContentCoding};
use enumset::EnumSet;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// Request-decompression configuration.
///
/// The accepted codings are a single typed set listed as an array; an empty set
/// means inbound decompression is off, so there is no separate umbrella toggle.
#[derive(Debug, Clone, Deserialize, Serialize, TomlComment, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DecompressionConfig {
    /// The content codings the broker will decode request bodies from. An empty
    /// array disables inbound decompression entirely.
    #[serde(deserialize_with = "array_or_comma_list")]
    pub algorithms: EnumSet<ContentCoding>,
    /// Whether a body whose coding is not among [`algorithms`](Self::algorithms)
    /// is passed through untouched (`true`) rather than rejected (`false`).
    pub pass_through_unaccepted: bool,
}

impl Default for DecompressionConfig {
    fn default() -> DecompressionConfig {
        DecompressionConfig {
            algorithms: EnumSet::all(),
            pass_through_unaccepted: false,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{content_coding::ContentCoding, http::decompression::DecompressionConfig};
    use enumset::{EnumSet, enum_set};

    #[test]
    fn default_accepts_every_coding() {
        pretty_assertions::assert_eq!(DecompressionConfig::default().algorithms, EnumSet::<ContentCoding>::all());
    }

    #[test]
    fn pass_through_unaccepted_defaults_false() {
        assert!(!DecompressionConfig::default().pass_through_unaccepted);
    }

    #[test]
    fn algorithms_array_parses_from_toml() {
        let config: DecompressionConfig = toml::from_str("algorithms = [\"gzip\"]\npass_through_unaccepted = true").expect("decompression parses");
        pretty_assertions::assert_eq!(config.algorithms, enum_set!(ContentCoding::Gzip));
        assert!(config.pass_through_unaccepted);
    }

    #[test]
    fn decompression_config_round_trips_through_toml() {
        let config = DecompressionConfig {
            algorithms: ContentCoding::Brotli | ContentCoding::Deflate,
            pass_through_unaccepted: true,
        };
        let rendered = toml::to_string(&config).expect("decompression serialises");
        let parsed: DecompressionConfig = toml::from_str(&rendered).expect("decompression parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
