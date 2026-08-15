//! SPDX-FileCopyrightText: 2014-2025 Sean McArthur
//! SPDX-License-Identifier: MIT
//!
//! Typed `Prefer` request header for NGSI-LD (RFC 7240, clause 6.3.6).
//!
//! Portions of this file are derived from the disabled `Prefer` implementation
//! in <https://github.com/hyperium/headers>.
//! This file, including local modifications, is distributed under the MIT License.
//!
//! Parses the `Prefer` request header to extract NGSI-LD-specific preferences:
//! - `body=json` / `body=ld+json` for GeoJSON @context placement
//! - `ngsi-ld=<version>` for version conformance negotiation (clause 4.3.6.8)
//! - `respond-async` / `respond-sync` for asynchronous vs synchronous processing
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
//! * `respond-async` / `respond-sync` -- asynchronous vs synchronous processing

use crate::{
    geo_json_body::GeoJsonBodyPreference,
    preference::{Preference, RespondMode},
    version::NgsiLdVersion,
};
use http::{HeaderName, HeaderValue};
use std::iter;

/// Typed `Prefer` request header for NGSI-LD (RFC 7240, clause 6.3.6).
///
/// Contains only NGSI-LD-relevant preference tokens; unrecognized RFC 7240
/// tokens (e.g. `return=minimal`, `wait=10`) are silently discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefer(Vec<Preference>);

impl Prefer {
    /// Return the GeoJSON body preference, defaulting to `LdJson` if absent.
    pub fn geo_json_body(&self) -> GeoJsonBodyPreference {
        self.0
            .iter()
            .find_map(|p| match p {
                Preference::Body(body) => Some(*body),
                Preference::NgsiLdVersion(_) | Preference::Respond(_) => None,
            })
            .unwrap_or_default()
    }

    /// Return the requested NGSI-LD version, if present.
    pub fn requested_version(&self) -> Option<NgsiLdVersion> {
        self.0.iter().find_map(|p| match p {
            Preference::NgsiLdVersion(v) => Some(*v),
            Preference::Body(_) | Preference::Respond(_) => None,
        })
    }

    /// Return the requested response mode (`respond-async` / `respond-sync`),
    /// if the client included one.
    pub fn respond_mode(&self) -> Option<RespondMode> {
        self.0.iter().find_map(|p| match p {
            Preference::Respond(mode) => Some(*mode),
            Preference::Body(_) | Preference::NgsiLdVersion(_) => None,
        })
    }
}

/// The header name, as a `HeaderName` the `headers` trait can hand back.
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
            preferences.extend(s.split(',').map(str::trim).filter_map(Preference::parse));
        }
        Ok(Prefer(preferences))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let s: String = self.0.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
        match HeaderValue::from_str(&s) {
            Ok(val) => values.extend(iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode Prefer header"),
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
        let mut iter = iter::once(&val);
        Prefer::decode(&mut iter).expect("valid Prefer header")
    }

    #[test]
    fn decode_body_json() {
        let prefer = decode_prefer("body=json");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]));
    }

    #[test]
    fn decode_body_ld_json() {
        let prefer = decode_prefer("body=ld+json");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::LdJson)]));
    }

    #[test]
    fn decode_ngsild_version_1_0() {
        let prefer = decode_prefer("ngsi-ld=1.0");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_0)]));
    }

    #[test]
    fn decode_ngsild_version_1_9() {
        let prefer = decode_prefer("ngsi-ld=1.9");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_9)]));
    }

    #[test]
    fn decode_combined_preferences() {
        let prefer = decode_prefer("body=json, ngsi-ld=1.4");
        pretty_assertions::assert_eq!(
            prefer,
            Prefer(vec![
                Preference::Body(GeoJsonBodyPreference::Json),
                Preference::NgsiLdVersion(NgsiLdVersion::V1_4),
            ])
        );
    }

    #[test]
    fn decode_ignores_unknown_tokens() {
        let prefer = decode_prefer("wait=5, body=json");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]));
    }

    #[test]
    fn decode_respond_async() {
        let prefer = decode_prefer("respond-async");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::Respond(RespondMode::Async)]));
    }

    #[test]
    fn decode_respond_sync() {
        let prefer = decode_prefer("respond-sync, body=json");
        pretty_assertions::assert_eq!(
            prefer,
            Prefer(vec![Preference::Respond(RespondMode::Sync), Preference::Body(GeoJsonBodyPreference::Json)])
        );
    }

    #[test]
    fn decode_ignores_unknown_version() {
        let prefer = decode_prefer("ngsi-ld=2.5");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![]));
    }

    #[test]
    fn decode_case_insensitive_body() {
        let prefer = decode_prefer("BODY=JSON");
        pretty_assertions::assert_eq!(prefer, Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]));
    }

    #[test]
    fn encode_roundtrip_body_json() {
        let original = Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]);
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = Prefer::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(original, decoded);
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
        pretty_assertions::assert_eq!(original, decoded);
    }

    #[test]
    fn geo_json_body_defaults_to_ld_json() {
        let prefer = Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_9)]);
        pretty_assertions::assert_eq!(prefer.geo_json_body(), GeoJsonBodyPreference::LdJson);
    }

    #[test]
    fn geo_json_body_returns_json() {
        let prefer = Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]);
        pretty_assertions::assert_eq!(prefer.geo_json_body(), GeoJsonBodyPreference::Json);
    }

    #[test]
    fn requested_version_extracts_version() {
        let prefer = Prefer(vec![Preference::NgsiLdVersion(NgsiLdVersion::V1_3)]);
        pretty_assertions::assert_eq!(prefer.requested_version(), Some(NgsiLdVersion::V1_3));
    }

    #[test]
    fn requested_version_none_when_absent() {
        let prefer = Prefer(vec![Preference::Body(GeoJsonBodyPreference::Json)]);
        pretty_assertions::assert_eq!(prefer.requested_version(), None);
    }
}
