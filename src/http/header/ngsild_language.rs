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
    language::LanguageRange,
    quality::{Quality, QualityValue},
};
use axum::http::{HeaderName, HeaderValue};
use langtag::LangTagBuf;

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
    /// Return all parsed language preferences.
    pub fn preferences(&self) -> &[QualityValue<LanguageRange>] {
        &self.0
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
                let qv = parse_language_preference(trimmed).ok_or_else(headers::Error::invalid)?;
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
        if let Ok(val) = HeaderValue::from_str(&s) {
            values.extend(std::iter::once(val));
        }
    }
}

/// Parse a single language preference from a trimmed segment like `fr;q=0.9`.
fn parse_language_preference(raw: &str) -> Option<QualityValue<LanguageRange>> {
    let mut segments = raw.splitn(2, ';');
    let range_str = segments.next()?.trim();

    let range = if range_str == "*" {
        LanguageRange::Wildcard
    } else {
        let tag = LangTagBuf::new(range_str.to_string()).ok()?;
        LanguageRange::Tag(tag)
    };

    let quality = if let Some(params) = segments.next() {
        parse_quality_param(params.trim())?
    } else {
        Quality::default()
    };

    Some(QualityValue::new(range, quality))
}

/// Parse a `q=0.9` parameter string into a [`Quality`] value.
fn parse_quality_param(param: &str) -> Option<Quality> {
    let q_str = param.strip_prefix("q=").or_else(|| param.strip_prefix("Q="))?;

    let f: f32 = q_str.parse().ok()?;
    if !(0.0..=1.0).contains(&f) {
        return None;
    }
    Quality::new((f * 1000.0) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(header.0.len(), 1);
        assert_eq!(header.0[0].value().to_string(), "fr");
        assert_eq!(header.0[0].quality(), Quality::default());
    }

    #[test]
    fn decode_tag_with_region() {
        let header = decode_header("fr-CH").expect("valid");
        assert_eq!(header.0[0].value().to_string(), "fr-CH");
    }

    #[test]
    fn decode_wildcard() {
        let header = decode_header("*").expect("valid");
        assert_eq!(*header.0[0].value(), LanguageRange::Wildcard);
    }

    #[test]
    fn decode_multiple_with_quality() {
        let header = decode_header("fr-CH, fr;q=0.9, en;q=0.8, *;q=0.5").expect("valid");
        assert_eq!(header.0.len(), 4);
        assert_eq!(header.0[0].quality().value(), 1000);
        assert_eq!(header.0[1].quality().value(), 900);
        assert_eq!(header.0[2].quality().value(), 800);
        assert_eq!(header.0[3].quality().value(), 500);
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
        use crate::query::lang::LanguageFilter;

        let header = decode_header("en;q=0.5, fr;q=0.9, de").expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        assert_eq!(ordered[0].range().to_string(), "de");
        assert_eq!(ordered[1].range().to_string(), "fr");
        assert_eq!(ordered[2].range().to_string(), "en");
    }

    #[test]
    fn ordered_preserves_input_order_for_equal_quality() {
        use crate::query::lang::LanguageFilter;

        let header = decode_header("de, en, fr").expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        assert_eq!(ordered[0].range().to_string(), "de");
        assert_eq!(ordered[1].range().to_string(), "en");
        assert_eq!(ordered[2].range().to_string(), "fr");
    }

    #[test]
    fn ordered_excludes_zero_quality() {
        use crate::query::lang::LanguageFilter;

        let header = decode_header("de;q=0, en;q=0.8, *;q=0.5").expect("valid");
        let filter = LanguageFilter::from(header);
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].range().to_string(), "en");
        assert_eq!(ordered[1].range().to_string(), "*");
    }

    #[test]
    fn encode_roundtrip() {
        let original = decode_header("fr, en;q=0.8").expect("valid");
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = NgsiLdLanguage::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(original, decoded);
    }
}
