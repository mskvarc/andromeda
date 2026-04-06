//! Typed `Accept` request header for NGSI-LD content negotiation.
//!
//! Portions of this file are derived from [Headers](https://github.com/hyperium/headers).
//! Licensed under the MIT License.
//! Copyright (c) 2014-2025 Sean McArthur.
//!
//! Parses `Accept` header values into quality-ranked NGSI-LD response formats
//! per [RFC 7231 Section 5.3.2](https://datatracker.ietf.org/doc/html/rfc7231#section-5.3.2).
//!
//! Only recognizes media types relevant to NGSI-LD:
//! - `application/json`
//! - `application/ld+json`
//! - `application/geo+json`
//! - `*/*` (wildcard, resolves to `application/json`)

use crate::http::{
    extractor::accept::GeoJsonSupport,
    model::{
        quality::{Quality, QualityValue},
        response_format::ResponseFormat,
    },
};
use axum::http::{HeaderName, HeaderValue};
use mediatype::{MediaTypeList, ReadParams, names};

/// Parse a single media type into a supported [`ResponseFormat`], if recognized.
///
/// Matches:
/// - `application/ld+json` -> `JsonLd`
/// - `application/geo+json` -> `GeoJson`
/// - `application/json` (exact, no suffix) -> `Json`
/// - `*/*` -> `Json` (wildcard defaults to JSON per NGSI-LD convention)
fn match_media_type(mt: &mediatype::MediaType) -> Option<ResponseFormat> {
    // */* wildcard
    if mt.ty == names::_STAR && mt.subty == names::_STAR {
        return Some(ResponseFormat::Json);
    }

    if mt.ty != names::APPLICATION {
        return None;
    }

    // application/ld+json -- subtype=ld, suffix=json
    if mt.subty == names::LD && mt.suffix == Some(names::JSON) {
        return Some(ResponseFormat::JsonLd);
    }

    // application/geo+json -- subtype=geo, suffix=json
    if mt.subty == names::GEO && mt.suffix == Some(names::JSON) {
        return Some(ResponseFormat::GeoJson);
    }

    // application/json -- exact match, no suffix (prevents application/merge-patch+json)
    if mt.subty == names::JSON && mt.suffix.is_none() {
        return Some(ResponseFormat::Json);
    }

    None
}

/// Extract the `q` quality parameter from a media type (defaults to 1.0 per RFC 7231).
fn extract_quality(mt: &mediatype::MediaType) -> Quality {
    mt.get_param(names::Q).and_then(|v| Quality::parse_decimal(v.as_str()).ok()).unwrap_or_default()
}

/// Typed `Accept` request header for NGSI-LD content negotiation.
///
/// Parses `Accept` header values into quality-ranked NGSI-LD response formats.
/// Only recognizes media types relevant to NGSI-LD:
/// - `application/json`
/// - `application/ld+json`
/// - `application/geo+json`
/// - `*/*` (wildcard, resolves to `application/json`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accept(Vec<QualityValue<ResponseFormat>>);

impl Accept {
    /// Return the highest-quality supported format.
    ///
    /// Filters out GeoJSON when `geo_json` is [`GeoJsonSupport::Disallowed`].
    /// Returns `None` when no supported format is found (caller maps this to 406).
    pub fn best_format(&self, geo_json: GeoJsonSupport) -> Option<ResponseFormat> {
        let mut best: Option<&QualityValue<ResponseFormat>> = None;
        for qv in &self.0 {
            if geo_json == GeoJsonSupport::Disallowed && *qv.value() == ResponseFormat::GeoJson {
                continue;
            }
            match best {
                None => best = Some(qv),
                Some(current) if qv.quality() > current.quality() => best = Some(qv),
                _ => {}
            }
        }
        best.map(|qv| *qv.value())
    }
}

static ACCEPT: HeaderName = HeaderName::from_static("accept");

impl headers::Header for Accept {
    fn name() -> &'static HeaderName {
        &ACCEPT
    }

    fn decode<'i, I>(values: &mut I) -> Result<Accept, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut formats = Vec::new();
        for value in values {
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            for result in MediaTypeList::new(s) {
                let Ok(mt) = result else { continue };
                if let Some(format) = match_media_type(&mt) {
                    let quality = extract_quality(&mt);
                    formats.push(QualityValue::new(format, quality));
                }
            }
        }
        if formats.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(Accept(formats))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let s: String = self.0.iter().map(|qv| qv.to_string()).collect::<Vec<_>>().join(", ");
        match HeaderValue::from_str(&s) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode Accept header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    /// Helper: decode an `Accept` from a raw header string.
    fn decode_accept(raw: &str) -> Result<Accept, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        Accept::decode(&mut iter)
    }

    #[test]
    fn decode_json() {
        let accept = decode_accept("application/json").expect("valid");
        assert_eq!(accept.0.len(), 1);
        assert_eq!(*accept.0[0].value(), ResponseFormat::Json);
        assert_eq!(accept.0[0].quality(), Quality::default());
    }

    #[test]
    fn decode_jsonld() {
        let accept = decode_accept("application/ld+json").expect("valid");
        assert_eq!(accept.0.len(), 1);
        assert_eq!(*accept.0[0].value(), ResponseFormat::JsonLd);
    }

    #[test]
    fn decode_geojson() {
        let accept = decode_accept("application/geo+json").expect("valid");
        assert_eq!(accept.0.len(), 1);
        assert_eq!(*accept.0[0].value(), ResponseFormat::GeoJson);
    }

    #[test]
    fn decode_wildcard_becomes_json() {
        let accept = decode_accept("*/*").expect("valid");
        assert_eq!(accept.best_format(GeoJsonSupport::Allowed), Some(ResponseFormat::Json));
    }

    #[test]
    fn decode_with_quality() {
        let accept = decode_accept("application/ld+json;q=0.9, application/json").expect("valid");
        // Json has q=1.0, JsonLd has q=0.9 -> Json wins
        assert_eq!(accept.best_format(GeoJsonSupport::Allowed), Some(ResponseFormat::Json));
    }

    #[test]
    fn decode_prefers_higher_quality() {
        let accept = decode_accept("application/json;q=0.5, application/ld+json;q=1.0").expect("valid");
        assert_eq!(accept.best_format(GeoJsonSupport::Allowed), Some(ResponseFormat::JsonLd));
    }

    #[test]
    fn decode_ignores_unsupported() {
        let accept = decode_accept("application/xml, application/json").expect("valid");
        assert_eq!(accept.best_format(GeoJsonSupport::Allowed), Some(ResponseFormat::Json));
    }

    #[test]
    fn decode_json_patch_not_matched() {
        let result = decode_accept("application/json-patch+json");
        assert!(result.is_err());
    }

    #[test]
    fn decode_merge_patch_not_matched() {
        let result = decode_accept("application/merge-patch+json");
        assert!(result.is_err());
    }

    #[test]
    fn best_format_excludes_geojson_when_disallowed() {
        let accept = decode_accept("application/geo+json").expect("valid");
        assert_eq!(accept.best_format(GeoJsonSupport::Disallowed), None);
    }

    #[test]
    fn best_format_allows_geojson_when_allowed() {
        let accept = decode_accept("application/geo+json").expect("valid");
        assert_eq!(accept.best_format(GeoJsonSupport::Allowed), Some(ResponseFormat::GeoJson));
    }

    #[test]
    fn encode_roundtrip() {
        let original = decode_accept("application/json").expect("valid");
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = Accept::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(original, decoded);
    }
}
