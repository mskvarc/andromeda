//! Prefer header extraction per RFC 7240 and NGSI-LD clause 6.3.6.
//!
//! Parses the `Prefer` request header to extract:
//! - `body=json` / `body=ld+json` for GeoJSON @context placement
//! - `ngsi-ld=<version>` for version conformance negotiation

use axum::{extract::FromRequestParts, http::request::Parts};
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
    pub fn as_str(&self) -> &'static str {
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

/// Parsed Prefer request header per RFC 7240 and NGSI-LD clause 6.3.6.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreferHeader {
    /// GeoJSON body preference: whether @context goes in body or Link header.
    pub geo_json_body: GeoJsonBodyPreference,
    /// Requested NGSI-LD version for backwards-compatible payload adaptation.
    pub requested_version: Option<NgsiLdVersion>,
}

/// Parse the Prefer header value into a `PreferHeader`.
///
/// Tokens are comma-separated. Unrecognized tokens are silently ignored
/// per RFC 7240 (preferences are advisory hints).
fn parse_prefer(header_value: &str) -> PreferHeader {
    let mut result = PreferHeader::default();

    for token in header_value.split(',').map(str::trim) {
        if token.eq_ignore_ascii_case("body=json") {
            result.geo_json_body = GeoJsonBodyPreference::Json;
        } else if token.eq_ignore_ascii_case("body=ld+json") {
            result.geo_json_body = GeoJsonBodyPreference::LdJson;
        } else if let Some(version_str) = token.strip_prefix("ngsi-ld=")
            && let Some(version) = NgsiLdVersion::parse(version_str)
        {
            result.requested_version = Some(version);
        }
    }

    result
}

impl<S> FromRequestParts<S> for PreferHeader
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let prefer = parts.headers.get("Prefer").and_then(|v| v.to_str().ok()).map(parse_prefer).unwrap_or_default();
        Ok(prefer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    fn parse_from_header(value: Option<&str>) -> PreferHeader {
        let mut builder = Request::builder().uri("/test");
        if let Some(v) = value {
            builder = builder.header("Prefer", v);
        }
        let req = builder.body(()).unwrap();
        let (parts, _) = req.into_parts();
        parts.headers.get("Prefer").and_then(|v| v.to_str().ok()).map(parse_prefer).unwrap_or_default()
    }

    #[test]
    fn absent_header_returns_defaults() {
        let prefer = parse_from_header(None);
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn body_json_preference() {
        let prefer = parse_from_header(Some("body=json"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
        assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn body_ld_json_preference() {
        let prefer = parse_from_header(Some("body=ld+json"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn version_1_0() {
        let prefer = parse_from_header(Some("ngsi-ld=1.0"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_0));
    }

    #[test]
    fn version_1_3() {
        let prefer = parse_from_header(Some("ngsi-ld=1.3"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_3));
    }

    #[test]
    fn version_1_9() {
        let prefer = parse_from_header(Some("ngsi-ld=1.9"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_9));
    }

    #[test]
    fn combined_preferences() {
        let prefer = parse_from_header(Some("body=json, ngsi-ld=1.4"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
        assert_eq!(prefer.requested_version, Some(NgsiLdVersion::V1_4));
    }

    #[test]
    fn unknown_version_ignored() {
        let prefer = parse_from_header(Some("ngsi-ld=2.5"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::LdJson);
        assert_eq!(prefer.requested_version, None);
    }

    #[test]
    fn unknown_token_ignored() {
        let prefer = parse_from_header(Some("respond-async, body=json"));
        assert_eq!(prefer.geo_json_body, GeoJsonBodyPreference::Json);
        assert_eq!(prefer.requested_version, None);
    }
}
