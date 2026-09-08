//! SPDX-FileCopyrightText: 2014-2025 Sean `McArthur`
//! SPDX-License-Identifier: MIT
//!
//! Typed `NGSILD-Language` request header for language negotiation.
//!
//! Portions of this file are derived from the disabled `AcceptLanguage`
//! implementation in <https://github.com/hyperium/headers>.
//! This file, including local modifications, is distributed under the MIT License.
//!
//! Parses the `NGSILD-Language` header value using the same syntax as
//! `Accept-Language` ([RFC 7231 Section 5.3.5](https://datatracker.ietf.org/doc/html/rfc7231#section-5.3.5)):
//! comma-separated BCP 47 language ranges with optional quality values.
//!
//! This is an Andromeda-specific header that behaves identically to the
//! `lang` query parameter (NGSI-LD clause 4.15) but transmitted as a
//! request header instead of a query parameter.

use crate::language::{LanguageFilter, LanguageRange};
use http::{HeaderName, HeaderValue};
use std::iter;

/// Typed `NGSILD-Language` request header.
///
/// Holds the parsed [`LanguageFilter`] rather than a raw preference list, so
/// the filter's non-emptiness invariant is established once, by the decoder
/// that can actually reject an empty header.
///
/// # Example header values
///
/// * `fr-CH, fr;q=0.9, en;q=0.8, *;q=0.5`
/// * `de, en-US;q=0.7`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdLanguage(LanguageFilter);

impl NgsiLdLanguage {
    /// The negotiated language preferences this header carries.
    #[must_use]
    pub fn into_filter(self) -> LanguageFilter {
        self.0
    }
}

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_LANGUAGE: HeaderName = HeaderName::from_static("ngsild-language");

impl headers::Header for NgsiLdLanguage {
    fn name() -> &'static HeaderName {
        &NGSILD_LANGUAGE
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdLanguage, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut preferences = Vec::new();
        for value in values {
            let raw = value.to_str().map_err(|_| headers::Error::invalid())?;
            for item in raw.split(',') {
                let trimmed = item.trim();
                if trimmed.is_empty() {
                    continue;
                }
                preferences.push(LanguageRange::parse_ranked(trimmed).map_err(|_| headers::Error::invalid())?);
            }
        }
        LanguageFilter::new(preferences).map(NgsiLdLanguage).map_err(|_| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let rendered: String = self.0.preferences().iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        match HeaderValue::from_str(&rendered) {
            Ok(value) => values.extend(iter::once(value)),
            Err(error) => tracing::warn!(error = %error, "failed to encode NGSILD-Language header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{language::LanguageRange, ngsild_language::NgsiLdLanguage, quality::Quality};
    use headers::Header;
    use http::HeaderValue;
    use std::iter;

    /// Decode an `NgsiLdLanguage` from a raw header string.
    fn decode_header(raw: &str) -> Result<NgsiLdLanguage, headers::Error> {
        let value = HeaderValue::from_str(raw).expect("valid header value");
        let mut values = iter::once(&value);
        NgsiLdLanguage::decode(&mut values)
    }

    #[test]
    fn a_single_tag_decodes_at_the_default_quality() {
        let filter = decode_header("fr").expect("valid").into_filter();
        pretty_assertions::assert_eq!(filter.preferences().len(), 1);
        pretty_assertions::assert_eq!(filter.preferences()[0].value().to_string(), "fr");
        pretty_assertions::assert_eq!(filter.preferences()[0].quality(), Quality::default());
    }

    #[test]
    fn a_tag_with_a_region_keeps_its_subtag() {
        let filter = decode_header("fr-CH").expect("valid").into_filter();
        pretty_assertions::assert_eq!(filter.preferences()[0].value().to_string(), "fr-CH");
    }

    #[test]
    fn a_wildcard_decodes_to_the_wildcard_range() {
        let filter = decode_header("*").expect("valid").into_filter();
        pretty_assertions::assert_eq!(*filter.preferences()[0].value(), LanguageRange::Wildcard);
    }

    #[test]
    fn each_preference_keeps_the_quality_it_was_given() {
        let filter = decode_header("fr-CH, fr;q=0.9, en;q=0.8, *;q=0.5").expect("valid").into_filter();
        let qualities: Vec<u16> = filter.preferences().iter().map(|preference| preference.quality().value()).collect();
        pretty_assertions::assert_eq!(qualities, vec![1000, 900, 800, 500]);
    }

    #[test]
    fn a_header_that_names_no_language_is_rejected() {
        assert!(decode_header("").is_err());
        // A bare number is not a valid BCP 47 tag.
        assert!(decode_header("123").is_err());
    }

    #[test]
    fn the_filter_orders_by_quality_then_wire_order() {
        let filter = decode_header("en;q=0.5, fr;q=0.9, de").expect("valid").into_filter();
        let rendered: Vec<String> = filter.ordered().iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["de", "fr", "en"]);
    }

    #[test]
    fn equal_quality_preserves_the_order_written() {
        let filter = decode_header("de, en, fr").expect("valid").into_filter();
        let rendered: Vec<String> = filter.ordered().iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["de", "en", "fr"]);
    }

    #[test]
    fn a_zero_quality_preference_is_not_offered() {
        let filter = decode_header("de;q=0, en;q=0.8, *;q=0.5").expect("valid").into_filter();
        let rendered: Vec<String> = filter.ordered().iter().map(|preference| preference.value().to_string()).collect();
        pretty_assertions::assert_eq!(rendered, vec!["en", "*"]);
    }

    #[test]
    fn the_encoding_decodes_back_to_the_same_header() {
        let original = decode_header("fr, en;q=0.8").expect("valid");
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut values = encoded.iter();
        let decoded = NgsiLdLanguage::decode(&mut values).expect("roundtrip decode");
        pretty_assertions::assert_eq!(original, decoded);
    }
}
