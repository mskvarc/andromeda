//! NGSI-LD preference types shared between Prefer and Preference-Applied headers.

use crate::http::model::version::NgsiLdVersion;
use std::fmt;

/// Controls whether @context appears in GeoJSON body or Link header (clause 6.3.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GeoJsonBodyPreference {
    /// `body=ld+json` or absent: @context included in response body.
    #[default]
    LdJson,
    /// `body=json`: @context in Link header, omitted from body.
    Json,
}

/// Individual NGSI-LD preference token (clause 6.3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    /// `body=json` or `body=ld+json` -- GeoJSON @context placement (clause 6.3.6).
    Body(GeoJsonBodyPreference),
    /// `ngsi-ld=<version>` -- request version conformance (clause 4.3.6.8).
    NgsiLdVersion(NgsiLdVersion),
}

// Manual Display impl: the format `body=json`, `ngsi-ld=1.9` does not map to
// any strum serialization pattern, so derive is not applicable here.
impl fmt::Display for Preference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Preference::Body(GeoJsonBodyPreference::Json) => f.write_str("body=json"),
            Preference::Body(GeoJsonBodyPreference::LdJson) => f.write_str("body=ld+json"),
            Preference::NgsiLdVersion(version) => write!(f, "ngsi-ld={version}"),
        }
    }
}

/// Parse a single preference token from a trimmed header value segment.
///
/// Returns `None` for unrecognized tokens (per RFC 7240, preferences are advisory hints).
pub fn parse_preference(token: &str) -> Option<Preference> {
    if token.eq_ignore_ascii_case("body=json") {
        Some(Preference::Body(GeoJsonBodyPreference::Json))
    } else if token.eq_ignore_ascii_case("body=ld+json") {
        Some(Preference::Body(GeoJsonBodyPreference::LdJson))
    } else if let Some(version_str) = token.strip_prefix("ngsi-ld=")
        && let Some(version) = NgsiLdVersion::parse(version_str)
    {
        Some(Preference::NgsiLdVersion(version))
    } else {
        None
    }
}
