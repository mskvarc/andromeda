//! SPDX-FileCopyrightText: 2014-2025 Sean `McArthur`
//! SPDX-License-Identifier: MIT
//!
//! Typed `Preference-Applied` response header for NGSI-LD (RFC 7240, clause 6.3.6).
//!
//! Portions of this file are derived from the disabled `PreferenceApplied`
//! implementation in <https://github.com/hyperium/headers>.
//! This file, including local modifications, is distributed under the MIT License.
//!
//! The `Preference-Applied` response header may be included within a
//! response message as an indication as to which `Prefer` header tokens were
//! honored by the server and applied to the processing of a request.
//!
//! # ABNF
//!
//! ```text
//! Preference-Applied = "Preference-Applied" ":" 1#applied-pref
//! applied-pref = token [ BWS "=" BWS word ]
//! ```
//!
//! # NGSI-LD example values
//!
//! * `ngsi-ld=1.9` -- server conforms to NGSI-LD v1.9
//! * `body=json` -- server applied JSON body preference

use crate::preference::{Preference, RespondMode};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use http::{HeaderName, HeaderValue};
use std::iter;

/// Typed `Preference-Applied` response header for NGSI-LD (RFC 7240, clause 6.3.6).
///
/// Contains the preference tokens that the server actually applied when
/// processing the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceApplied(Vec<Preference>);

impl PreferenceApplied {
    /// A `Preference-Applied: ngsi-ld=<version>` naming the NGSI-LD version the
    /// response payload conforms to (clauses 4.3.6.8 and 6.3.6): the version
    /// the payload was amended down to, or the version the server itself
    /// conforms to when no amendment was needed.
    #[must_use]
    pub fn version(version: SpecVersion) -> PreferenceApplied {
        PreferenceApplied(vec![Preference::Version(version)])
    }

    /// Create a `Preference-Applied: respond-async` header, signalling the
    /// server accepted the client's async processing hint for this request.
    #[must_use]
    pub fn respond_async() -> PreferenceApplied {
        PreferenceApplied(vec![Preference::Respond(RespondMode::Async)])
    }

    /// The NGSI-LD version the server says its payload conforms to (clause
    /// 4.3.6.8), when the header names one.
    #[must_use]
    pub fn applied_version(&self) -> Option<SpecVersion> {
        self.0.iter().find_map(|preference| match preference {
            Preference::Version(version) => Some(*version),
            Preference::Body(_) | Preference::Respond(_) => None,
        })
    }
}

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static PREFERENCE_APPLIED: HeaderName = HeaderName::from_static("preference-applied");

impl headers::Header for PreferenceApplied {
    fn name() -> &'static HeaderName {
        &PREFERENCE_APPLIED
    }

    fn decode<'i, I>(values: &mut I) -> Result<PreferenceApplied, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut preferences = Vec::new();
        for value in values {
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            preferences.extend(s.split(',').map(str::trim).filter_map(Preference::parse));
        }
        Ok(PreferenceApplied(preferences))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let s: String = self.0.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        match HeaderValue::from_str(&s) {
            Ok(val) => values.extend(iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode Preference-Applied header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    /// Helper: decode a `PreferenceApplied` from a raw header string.
    fn decode_preference_applied(raw: &str) -> PreferenceApplied {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = iter::once(&val);
        PreferenceApplied::decode(&mut iter).expect("valid Preference-Applied header")
    }

    #[test]
    fn decode_ngsild_version() {
        let pa = decode_preference_applied("ngsi-ld=1.9");
        pretty_assertions::assert_eq!(pa, PreferenceApplied(vec![Preference::Version(SpecVersion::V1_9)]));
    }

    #[test]
    fn encode_version_names_the_given_version() {
        let pa = PreferenceApplied::version(SpecVersion::V1_8);
        let mut encoded = Vec::new();
        pa.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "ngsi-ld=1.8");
    }

    #[test]
    fn encode_roundtrip() {
        let original = PreferenceApplied::version(SpecVersion::CONFORMANT);
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = PreferenceApplied::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(original, decoded);
    }

    #[test]
    fn the_applied_version_is_read_back_from_a_decoded_header() {
        pretty_assertions::assert_eq!(decode_preference_applied("ngsi-ld=1.3").applied_version(), Some(SpecVersion::V1_3));
        pretty_assertions::assert_eq!(PreferenceApplied::respond_async().applied_version(), None);
    }

    #[test]
    fn the_version_constructor_reads_back_as_the_applied_version() {
        let pa = PreferenceApplied::version(SpecVersion::CONFORMANT);
        pretty_assertions::assert_eq!(pa, PreferenceApplied(vec![Preference::Version(SpecVersion::CONFORMANT)]));
        pretty_assertions::assert_eq!(pa.applied_version(), Some(SpecVersion::CONFORMANT));
    }
}
