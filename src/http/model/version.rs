//! NGSI-LD specification version types for backwards compatibility negotiation.

use std::fmt;

/// NGSI-LD specification version for backwards compatibility (clause 4.3.6.8).
///
/// Variant order encodes version ordering: earlier variants represent older versions.
/// Derived `PartialOrd`/`Ord` compare by discriminant, so `V1_0 < V1_3 < V1_4 < V1_8 < V1_9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NgsiLdVersion {
    /// NGSI-LD v1.0
    V1_0,
    /// NGSI-LD v1.3
    V1_3,
    /// NGSI-LD v1.4
    V1_4,
    /// NGSI-LD v1.8
    V1_8,
    /// NGSI-LD v1.9
    V1_9,
}

impl NgsiLdVersion {
    /// The version that Andromeda conforms to.
    pub const CONFORMANT: NgsiLdVersion = NgsiLdVersion::V1_9;

    /// Return the version string as it appears in the Prefer/Preference-Applied header.
    pub const fn as_str(&self) -> &'static str {
        match self {
            NgsiLdVersion::V1_0 => "1.0",
            NgsiLdVersion::V1_3 => "1.3",
            NgsiLdVersion::V1_4 => "1.4",
            NgsiLdVersion::V1_8 => "1.8",
            NgsiLdVersion::V1_9 => "1.9",
        }
    }

    /// Parse a version string from a header value.
    ///
    /// Returns `None` for unrecognized versions.
    pub fn parse(s: &str) -> Option<NgsiLdVersion> {
        match s {
            "1.0" => Some(NgsiLdVersion::V1_0),
            "1.3" => Some(NgsiLdVersion::V1_3),
            "1.4" => Some(NgsiLdVersion::V1_4),
            "1.8" => Some(NgsiLdVersion::V1_8),
            "1.9" => Some(NgsiLdVersion::V1_9),
            _ => None,
        }
    }
}

impl fmt::Display for NgsiLdVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ngsild_version_ordering() {
        assert!(NgsiLdVersion::V1_0 < NgsiLdVersion::V1_3);
        assert!(NgsiLdVersion::V1_3 < NgsiLdVersion::V1_4);
        assert!(NgsiLdVersion::V1_4 < NgsiLdVersion::V1_8);
        assert!(NgsiLdVersion::V1_8 < NgsiLdVersion::V1_9);
    }

    #[test]
    fn ngsild_version_display() {
        assert_eq!(NgsiLdVersion::V1_9.to_string(), "1.9");
    }

    #[test]
    fn ngsild_version_parse() {
        assert_eq!(NgsiLdVersion::parse("1.4"), Some(NgsiLdVersion::V1_4));
    }

    #[test]
    fn ngsild_version_parse_unknown() {
        assert_eq!(NgsiLdVersion::parse("2.0"), None);
    }
}
