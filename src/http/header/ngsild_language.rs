//! Typed `NGSILD-Language` request header for language negotiation.
//!
//! Portions of this file are derived from [Headers](https://github.com/hyperium/headers).
//! Licensed under the MIT License.
//! Copyright (c) 2014-2025 Sean McArthur.
//!
//! Parses the `NGSILD-Language` header value using the same syntax as
//! `Accept-Language` ([RFC 7231 Section 5.3.5](https://datatracker.ietf.org/doc/html/rfc7231#section-5.3.5)):
//! comma-separated BCP 47 language ranges with optional quality values.
//!
//! This is an Andromeda-specific header that behaves identically to the
//! `lang` query parameter (NGSI-LD clause 6.3.5) but transmitted as a
//! request header instead of a query parameter.

use crate::http::model::{
    language::{LanguageFilter, LanguageRange, parse_preference},
    quality::QualityValue,
};
use axum::http::{HeaderName, HeaderValue};

/// Typed `NGSILD-Language` request header.
///
/// Contains quality-ranked BCP 47 language preferences using the same syntax
/// as `Accept-Language` (RFC 7231 Section 5.3.5). This is an Andromeda-specific
/// header that semantically behaves the same as the `lang` query parameter.
///
/// # Example header values
///
/// * `fr-CH, fr;q=0.9, en;q=0.8, *;q=0.5`
/// * `de, en-US;q=0.7`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdLanguage(Vec<QualityValue<LanguageRange>>);

impl NgsiLdLanguage {
    /// Consume the header and return the inner preference list.
    pub fn into_preferences(self) -> Vec<QualityValue<LanguageRange>> {
        self.0
    }
}

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
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            for item in s.split(',') {
                let trimmed = item.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let qv = parse_preference(trimmed).map_err(|_| headers::Error::invalid())?;
                preferences.push(qv);
            }
        }
        if preferences.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(NgsiLdLanguage(preferences))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let s: String = self.0.iter().map(|qv| qv.to_string()).collect::<Vec<_>>().join(", ");
        match HeaderValue::from_str(&s) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Language header"),
        }
    }
}

impl From<NgsiLdLanguage> for LanguageFilter {
    fn from(header: NgsiLdLanguage) -> LanguageFilter {
        // NgsiLdLanguage always has at least one preference (decode rejects empty),
        // so bypassing LanguageFilter::new() validation is safe.
        LanguageFilter::from_nonempty(header.into_preferences())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::model::quality::Quality;
    use headers::Header;

    /// Helper: decode an `NgsiLdLanguage` from a raw header string.
    fn decode_header(raw: &str) -> Result<NgsiLdLanguage, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdLanguage::decode(&mut iter)
    }

    #[test]
    fn decode_single_tag() {
        let header = decode_header("fr").expect("valid");
        pretty_assertions::assert_eq!(header.0.len(), 1);
        pretty_assertions::assert_eq!(header.0[0].value().to_string(), "fr");
        pretty_assertions::assert_eq!(header.0[0].quality(), Quality::default());
    }

    #[test]
    fn decode_tag_with_region() {
        let header = decode_header("fr-CH").expect("valid");
        pretty_assertions::assert_eq!(header.0[0].value().to_string(), "fr-CH");
    }

    #[test]
    fn decode_wildcard() {
        let header = decode_header("*").expect("valid");
        pretty_assertions::assert_eq!(*header.0[0].value(), LanguageRange::Wildcard);
    }

    #[test]
    fn decode_multiple_with_quality() {
        let header = decode_header("fr-CH, fr;q=0.9, en;q=0.8, *;q=0.5").expect("valid");
        pretty_assertions::assert_eq!(header.0.len(), 4);
        pretty_assertions::assert_eq!(header.0[0].quality().value(), 1000);
        pretty_assertions::assert_eq!(header.0[1].quality().value(), 900);
        pretty_assertions::assert_eq!(header.0[2].quality().value(), 800);
        pretty_assertions::assert_eq!(header.0[3].quality().value(), 500);
    }

    #[test]
    fn decode_rejects_empty() {
        assert!(decode_header("").is_err());
    }

    #[test]
    fn decode_rejects_invalid_tag() {
        // A bare number is not a valid BCP 47 tag
        assert!(decode_header("123").is_err());
    }

    #[test]
    fn ordered_via_language_filter() {
        use crate::http::model::language::LanguageFilter;

        let header = decode_header("en;q=0.5, fr;q=0.9, de").expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered[0].value().to_string(), "de");
        pretty_assertions::assert_eq!(ordered[1].value().to_string(), "fr");
        pretty_assertions::assert_eq!(ordered[2].value().to_string(), "en");
    }

    #[test]
    fn ordered_preserves_input_order_for_equal_quality() {
        use crate::http::model::language::LanguageFilter;

        let header = decode_header("de, en, fr").expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered[0].value().to_string(), "de");
        pretty_assertions::assert_eq!(ordered[1].value().to_string(), "en");
        pretty_assertions::assert_eq!(ordered[2].value().to_string(), "fr");
    }

    #[test]
    fn ordered_excludes_zero_quality() {
        use crate::http::model::language::LanguageFilter;

        let header = decode_header("de;q=0, en;q=0.8, *;q=0.5").expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered.len(), 2);
        pretty_assertions::assert_eq!(ordered[0].value().to_string(), "en");
        pretty_assertions::assert_eq!(ordered[1].value().to_string(), "*");
    }

    #[test]
    fn encode_roundtrip() {
        let original = decode_header("fr, en;q=0.8").expect("valid");
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = NgsiLdLanguage::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(original, decoded);
    }
}
