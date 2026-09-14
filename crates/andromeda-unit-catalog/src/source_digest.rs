//! The digest identifying the artifact a catalog release was generated from.

use derive_more::Display;
use thiserror::Error;

/// Number of bytes in a SHA-256 digest.
const DIGEST_LENGTH: usize = 32;

/// The SHA-256 digest of the artifact a catalog release was generated from.
///
/// The catalog is only append-only with respect to a named input. Recording the
/// digest is what turns "regenerated from Recommendation 20" into a checkable
/// claim: a second release that assigns different identifiers must also name a
/// different artifact, and a release that names the same artifact must produce
/// the same assignment.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{}", hex::encode(_0))]
pub struct SourceDigest([u8; DIGEST_LENGTH]);

impl SourceDigest {
    /// Return the digest bytes.
    #[must_use]
    pub const fn get(self) -> [u8; DIGEST_LENGTH] {
        self.0
    }

    /// Return the lower-case hexadecimal rendering of the digest.
    #[must_use]
    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

impl TryFrom<&str> for SourceDigest {
    type Error = SourceDigestMalformed;

    fn try_from(hexadecimal: &str) -> Result<SourceDigest, SourceDigestMalformed> {
        let bytes = hex::decode(hexadecimal).map_err(|_| SourceDigestMalformed)?;
        let digest: [u8; DIGEST_LENGTH] = bytes.try_into().map_err(|_| SourceDigestMalformed)?;
        Ok(SourceDigest(digest))
    }
}

/// A digest was not thirty-two hexadecimal-encoded bytes.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a source digest is a SHA-256 value written as sixty-four hexadecimal characters")]
pub struct SourceDigestMalformed;

/// The digest of the Recommendation 20 workbook this catalog release was
/// generated from — the `rec20.xlsx` shipped by `cefact-units` 0.1.0.
pub const SOURCE_SHA256: SourceDigest = SourceDigest([
    0x99, 0x4a, 0x85, 0xc3, 0x68, 0x2e, 0x67, 0x60, 0x4d, 0x42, 0xdb, 0xda, 0x3b, 0x35, 0xd6, 0x72, 0x33, 0x8a, 0x45, 0xb4, 0x00, 0xf5, 0x39, 0x7a, 0x96, 0xa4,
    0xc5, 0x46, 0x7e, 0xe8, 0x05, 0x6d,
]);

#[cfg(test)]
mod tests {
    use crate::source_digest::{SOURCE_SHA256, SourceDigest, SourceDigestMalformed};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_release_digest_renders_as_the_workbook_checksum() {
        assert_eq!(SOURCE_SHA256.to_hex(), "994a85c3682e67604d42dbda3b35d672338a45b400f5397a96a4c5467ee8056d");
    }

    #[test]
    fn a_digest_round_trips_through_its_hexadecimal_rendering() {
        let rendered = SOURCE_SHA256.to_hex();
        assert_eq!(SourceDigest::try_from(rendered.as_str()), Ok(SOURCE_SHA256));
    }

    #[test]
    fn a_digest_of_the_wrong_length_is_refused() {
        assert_eq!(SourceDigest::try_from("994a85c3"), Err(SourceDigestMalformed));
    }

    #[test]
    fn a_digest_with_a_non_hexadecimal_character_is_refused() {
        let invalid = "z94a85c3682e67604d42dbda3b35d672338a45b400f5397a96a4c5467ee8056d";
        assert_eq!(SourceDigest::try_from(invalid), Err(SourceDigestMalformed));
    }
}
