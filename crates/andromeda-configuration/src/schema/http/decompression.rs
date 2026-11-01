//! Request decompression: which codings are decoded, and what happens to a
//! body in any other.

use crate::{
    comma_list::array_or_comma_list,
    schema::{content_coding::ContentCoding, feature_mode::FeatureMode},
};
use enumset::EnumSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// What happens to a request body whose content coding is not among the
/// decoded `algorithms`.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "What happens to a request body in a coding not listed in `http.decompression.algorithms`.")]
pub enum UnacceptedEncoding {
    /// The request is answered 415 Unsupported Media Type.
    #[default]
    #[schemars(description = "Answers the request `415` Unsupported Media Type.")]
    Refused,
    /// The body is handed on still encoded.
    #[schemars(description = "Passes the body on still encoded.")]
    Allowed,
}

/// Request-decompression configuration.
///
/// `mode` turns decompression on or off; the codings are a typed set that
/// must name at least one coding while decompression is on.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Request decompression: which codings a request body may arrive in, and what happens to one in any other.")]
pub struct DecompressionConfig {
    /// Whether request bodies are decoded at all.
    #[schemars(
        description = "Whether request bodies in a content coding are decoded. Off, every body is read as it arrives, and `http.decompression.unaccepted_encoding` does not apply."
    )]
    pub mode: FeatureMode,
    /// The content codings request bodies are decoded from.
    #[serde(deserialize_with = "array_or_comma_list")]
    #[schemars(
        with = "Vec<ContentCoding>",
        description = "The codings a request body may arrive in, decoded before the body is read. Read only when `http.decompression.mode` is `\"enabled\"`."
    )]
    pub algorithms: EnumSet<ContentCoding>,
    /// What happens to a body in any other coding: `refused` or `allowed`
    /// through still encoded.
    #[schemars(description = "What happens to a body in a coding that is not listed. Read only when `http.decompression.mode` is `\"enabled\"`.")]
    pub unaccepted_encoding: UnacceptedEncoding,
}

impl Default for DecompressionConfig {
    fn default() -> DecompressionConfig {
        DecompressionConfig {
            mode: FeatureMode::Enabled,
            algorithms: EnumSet::all(),
            unaccepted_encoding: UnacceptedEncoding::Refused,
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
        http::decompression::{DecompressionConfig, UnacceptedEncoding},
    };
    use enumset::{EnumSet, enum_set};

    #[test]
    fn the_default_decodes_every_coding_and_refuses_the_rest() {
        let config = DecompressionConfig::default();
        pretty_assertions::assert_eq!(config.mode, FeatureMode::Enabled);
        pretty_assertions::assert_eq!(config.algorithms, EnumSet::<ContentCoding>::all());
        pretty_assertions::assert_eq!(config.unaccepted_encoding, UnacceptedEncoding::Refused);
    }

    #[test]
    fn the_table_parses_from_toml() {
        let config: DecompressionConfig =
            toml::from_str("mode = \"enabled\"\nalgorithms = [\"gzip\"]\nunaccepted_encoding = \"allowed\"").expect("decompression parses");
        pretty_assertions::assert_eq!(config.algorithms, enum_set!(ContentCoding::Gzip));
        pretty_assertions::assert_eq!(config.unaccepted_encoding, UnacceptedEncoding::Allowed);
    }

    #[test]
    fn decompression_config_round_trips_through_toml() {
        let config = DecompressionConfig {
            mode: FeatureMode::Disabled,
            algorithms: ContentCoding::Brotli | ContentCoding::Deflate,
            unaccepted_encoding: UnacceptedEncoding::Allowed,
        };
        let rendered = toml::to_string(&config).expect("decompression serialises");
        let parsed: DecompressionConfig = toml::from_str(&rendered).expect("decompression parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
