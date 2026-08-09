//! The specification version a client asks a response to conform to.

use strum::{Display, EnumString, IntoStaticStr};

/// NGSI-LD specification version for backwards compatibility (clause 4.3.6.8).
///
/// Variant order encodes version ordering: earlier variants represent older
/// versions. Derived `PartialOrd`/`Ord` compare by discriminant, so
/// `V1_0 < V1_3 < V1_4 < V1_8 < V1_9`.
///
/// The rendering is the token as it appears in a `Prefer` or
/// `Preference-Applied` header, and `FromStr` is its exact inverse: an
/// unrecognised version fails to parse, which RFC 7240 requires a server to
/// ignore rather than reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display, EnumString, IntoStaticStr)]
pub enum NgsiLdVersion {
    /// NGSI-LD v1.0
    #[strum(serialize = "1.0")]
    V1_0,
    /// NGSI-LD v1.3
    #[strum(serialize = "1.3")]
    V1_3,
    /// NGSI-LD v1.4
    #[strum(serialize = "1.4")]
    V1_4,
    /// NGSI-LD v1.8
    #[strum(serialize = "1.8")]
    V1_8,
    /// NGSI-LD v1.9
    #[strum(serialize = "1.9")]
    V1_9,
}

impl NgsiLdVersion {
    /// The version that Andromeda conforms to.
    pub const CONFORMANT: NgsiLdVersion = NgsiLdVersion::V1_9;

    /// The version token as a static string.
    ///
    /// [`Display`](std::fmt::Display) produces the same text at the cost of an
    /// allocation, which the header encoders cannot afford per response.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::version::NgsiLdVersion;
    use std::str::FromStr;

    #[test]
    fn variant_order_is_version_order() {
        assert!(NgsiLdVersion::V1_0 < NgsiLdVersion::V1_3);
        assert!(NgsiLdVersion::V1_3 < NgsiLdVersion::V1_4);
        assert!(NgsiLdVersion::V1_4 < NgsiLdVersion::V1_8);
        assert!(NgsiLdVersion::V1_8 < NgsiLdVersion::V1_9);
    }

    #[test]
    fn every_version_renders_and_parses_back() {
        for variant in [
            NgsiLdVersion::V1_0,
            NgsiLdVersion::V1_3,
            NgsiLdVersion::V1_4,
            NgsiLdVersion::V1_8,
            NgsiLdVersion::V1_9,
        ] {
            pretty_assertions::assert_eq!(NgsiLdVersion::from_str(variant.as_str()), Ok(variant));
            pretty_assertions::assert_eq!(variant.to_string(), variant.as_str());
        }
    }

    #[test]
    fn display_matches_the_header_token() {
        pretty_assertions::assert_eq!(NgsiLdVersion::V1_9.to_string(), "1.9");
    }

    #[test]
    fn an_unknown_version_does_not_parse() {
        assert!(NgsiLdVersion::from_str("2.0").is_err());
    }
}
