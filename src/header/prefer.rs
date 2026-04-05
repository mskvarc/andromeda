//! Typed `Prefer` request header for NGSI-LD (RFC 7240, clause 6.3.6).
//!
//! Portions of this file are derived from [Headers](https://github.com/hyperium/headers).
//! Licensed under the MIT License.
//! Copyright (c) 2014-2025 Sean McArthur.
//!
//! Parses the `Prefer` request header to extract NGSI-LD-specific preferences:
//! - `body=json` / `body=ld+json` for GeoJSON @context placement
//! - `ngsi-ld=<version>` for version conformance negotiation (clause 4.3.6.8)
//!
//! The `Prefer` header field can be used by a client to request that certain
//! behaviors be employed by a server while processing a request.
//!
//! # ABNF
//!
//! ```text
//! Prefer     = "Prefer" ":" 1#preference
//! preference = token [ BWS "=" BWS word ]
//!              *( OWS ";" [ OWS parameter ] )
//! parameter  = token [ BWS "=" BWS word ]
//! ```
//!
//! # NGSI-LD preference tokens
//! * `body=json` -- @context in Link header, omitted from GeoJSON body
//! * `body=ld+json` -- @context included in GeoJSON response body
//! * `ngsi-ld=1.9` -- request version conformance (clause 4.3.6.8)

use axum::http::{HeaderName, HeaderValue};
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
pub(crate) fn parse_preference(token: &str) -> Option<Preference> {
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

/// Typed `Prefer` request header for NGSI-LD (RFC 7240, clause 6.3.6).
///
/// Contains only NGSI-LD-relevant preference tokens; unrecognized RFC 7240
/// tokens (e.g. `respond-async`, `return=minimal`) are silently discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefer(Vec<Preference>);

impl Prefer {
    /// Return the GeoJSON body preference, defaulting to `LdJson` if absent.
    pub fn geo_json_body(&self) -> GeoJsonBodyPreference {
        self.0
            .iter()
            .find_map(|p| match p {
                Preference::Body(body) => Some(*body),
                Preference::NgsiLdVersion(_) => None,
            })
            .unwrap_or_default()
    }

    /// Return the requested NGSI-LD version, if present.
    pub fn requested_version(&self) -> Option<NgsiLdVersion> {
        self.0.iter().find_map(|p| match p {
            Preference::NgsiLdVersion(v) => Some(*v),
            Preference::Body(_) => None,
        })
    }
}

static PREFER: HeaderName = HeaderName::from_static("prefer");

impl headers::Header for Prefer {
    fn name() -> &'static HeaderName {
        &PREFER
    }

    fn decode<'i, I>(values: &mut I) -> Result<Prefer, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut preferences = Vec::new();
        for value in values {
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            for token in s.split(',').map(str::trim) {
                if let Some(pref) = parse_preference(token) {
                    preferences.push(pref);
                }
            }
        }
        Ok(Prefer(preferences))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let s: String = self.0.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
        if let Ok(val) = HeaderValue::from_str(&s) {
            values.extend(std::iter::once(val));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    /// Helper: decode a `Prefer` from a raw header string.
    fn decode_prefer(raw: &str) -> Prefer {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        Prefer::decode(&mut iter).expect("valid Prefer header")
    }

    #[test]
    fn decode_body_json() {
        let prefer = decode_prefer("body=json");
        assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]));
    }

    #[test]
    fn decode_body_ld_json() {
        let prefer = decode_prefer("body=ld+json");
        assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::LdJson)]));
    }

    #[test]
    fn decode_ngsild_version_1_0() {
        let prefer = decode_prefer("ngsi-ld=1.0");
        assert_eq!(prefer, Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_0)]));
    }

    #[test]
    fn decode_ngsild_version_1_9() {
        let prefer = decode_prefer("ngsi-ld=1.9");
        assert_eq!(prefer, Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_9)]));
    }

    #[test]
    fn decode_combined_preferences() {
        let prefer = decode_prefer("body=json, ngsi-ld=1.4");
        assert_eq!(
            prefer,
            Prefer(vec![
                Preference::Body(GeoJsonBodyPreference::Json),
                Preference::NgsiLdVersion(NgsiLdVersion::V1_4),
            ])
        );
    }

    #[test]
    fn decode_ignores_unknown_tokens() {
        let prefer = decode_prefer("respond-async, body=json");
        assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]));
    }

    #[test]
    fn decode_ignores_unknown_version() {
        let prefer = decode_prefer("ngsi-ld=2.5");
        assert_eq!(prefer, Prefer(vec![]));
    }

    #[test]
    fn decode_case_insensitive_body() {
        let prefer = decode_prefer("BODY=JSON");
        assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]));
    }

    #[test]
    fn encode_roundtrip_body_json() {
        let original = Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]);
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = Prefer::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn encode_roundtrip_combined() {
        let original = Prefer(vec![
            Preference::Body(GeoJsonBodyPreference::Json),
            Preference::NgsiLdVersion(NgsiLdVersion::V1_4),
        ]);
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = Prefer::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn geo_json_body_defaults_to_ld_json() {
        let prefer = Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_9)]);
        assert_eq!(prefer.geo_json_body(), GeoJsonBodyPreference::LdJson);
    }

    #[test]
    fn geo_json_body_returns_json() {
        let prefer = Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]);
        assert_eq!(prefer.geo_json_body(), GeoJsonBodyPreference::Json);
    }

    #[test]
    fn requested_version_extracts_version() {
        let prefer = Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_3)]);
        assert_eq!(prefer.requested_version(), Some(NgsiLdVersion::V1_3));
    }

    #[test]
    fn requested_version_none_when_absent() {
        let prefer = Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]);
        assert_eq!(prefer.requested_version(), None);
    }

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
