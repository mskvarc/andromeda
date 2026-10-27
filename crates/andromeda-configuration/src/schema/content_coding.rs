//! The HTTP content codings the broker may compress with or decompress from.

use enumset::EnumSetType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// One HTTP content coding, in the RFC 9110 section 8.4.1 registry sense.
///
/// The four codings the broker's compression and decompression layers support.
/// Membership is expressed as a typed [`EnumSet`](enumset::EnumSet) listed in the
/// configuration as a token array (`["gzip", "brotli"]`) rather than as four
/// boolean fields: the set makes a duplicate or contradictory selection
/// unrepresentable, iterates in a stable order, and is a single value the layer
/// builders can test. `#[enumset(serialize_repr = "list")]` is what renders the
/// set as that array of tokens instead of a packed bitmask integer.
#[derive(Debug, Display, EnumString, Deserialize, Serialize, EnumSetType, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[enumset(serialize_repr = "list")]
#[schemars(description = "One HTTP content coding the broker can compress with or decompress from.")]
pub enum ContentCoding {
    /// The `gzip` coding (RFC 1952).
    #[schemars(description = "The `gzip` coding.")]
    Gzip,
    /// The `br` Brotli coding (RFC 7932); spelled `brotli` in configuration.
    #[schemars(description = "The Brotli coding, `br` on the wire.")]
    Brotli,
    /// The `deflate` zlib coding (RFC 1950/1951).
    #[schemars(description = "The `deflate` (zlib) coding.")]
    Deflate,
    /// The `zstd` Zstandard coding (RFC 8878).
    #[schemars(description = "The `zstd` Zstandard coding.")]
    Zstd,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::content_coding::ContentCoding;
    use enumset::EnumSet;

    #[test]
    fn codings_parse_kebab_tokens() {
        for (token, coding) in [
            ("gzip", ContentCoding::Gzip),
            ("brotli", ContentCoding::Brotli),
            ("deflate", ContentCoding::Deflate),
            ("zstd", ContentCoding::Zstd),
        ] {
            pretty_assertions::assert_eq!(token.parse::<ContentCoding>().expect("token parses"), coding);
            pretty_assertions::assert_eq!(coding.to_string(), token);
        }
    }

    #[test]
    fn set_serializes_as_token_array() {
        let set: EnumSet<ContentCoding> = ContentCoding::Gzip | ContentCoding::Brotli;
        let value = serde_json::to_value(set).expect("set serialises");
        pretty_assertions::assert_eq!(value, serde_json::json!(["gzip", "brotli"]));
    }

    #[test]
    fn duplicate_codings_collapse() {
        // The `list` representation is a set: repeating a coding in the wire array
        // yields a single membership rather than two.
        let set: EnumSet<ContentCoding> = serde_json::from_value(serde_json::json!(["gzip", "gzip", "brotli"])).expect("set parses");
        pretty_assertions::assert_eq!(set.len(), 2);
        assert!(set.contains(ContentCoding::Gzip));
        assert!(set.contains(ContentCoding::Brotli));
    }
}
