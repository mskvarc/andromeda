//! Accept header extraction for NGSI-LD content negotiation.
//!
//! Uses the `mediatype` crate for structured media type parsing instead of
//! substring matching, preventing false positives like `application/json-patch+json`
//! matching `application/json`.

use crate::error::ExtractorRejection;
use axum::{
    extract::FromRequestParts,
    http::{header::ACCEPT, request::Parts},
};
use mediatype::{MediaTypeList, ReadParams, names};

/// Response format based on Accept header.
///
/// Per ETSI GS CIM 009:
/// - `application/json`: Entity without @context, context URL in Link header
/// - `application/ld+json`: Entity with @context as URL reference in body
/// - `application/geo+json`: GeoJSON FeatureCollection format
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseFormat {
    /// application/json - @context omitted from body, provided via Link header
    #[default]
    Json,
    /// application/ld+json - @context included in body as URL reference
    JsonLd,
    /// application/geo+json - GeoJSON FeatureCollection format
    GeoJson,
}

impl ResponseFormat {
    /// Get the Content-Type header value for this format.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::JsonLd => "application/ld+json",
            Self::GeoJson => "application/geo+json",
        }
    }

    /// Whether @context should be included in the response body.
    #[allow(dead_code)]
    pub fn include_context_in_body(&self) -> bool {
        matches!(self, Self::JsonLd)
    }
}

/// A parsed media type entry from the Accept header with its quality value.
struct AcceptEntry {
    format: MatchedFormat,
    quality: f32,
}

/// Which supported format a media type matched.
enum MatchedFormat {
    Json,
    JsonLd,
    GeoJson,
    Wildcard,
}

/// Parse a single media type into a matched format (if supported).
///
/// Matches:
/// - `application/ld+json` → JsonLd
/// - `application/geo+json` → GeoJson
/// - `application/json` (exact, no suffix) → Json
/// - `*/*` → Wildcard
fn match_media_type(mt: &mediatype::MediaType) -> Option<MatchedFormat> {
    // */* wildcard
    if mt.ty == names::_STAR && mt.subty == names::_STAR {
        return Some(MatchedFormat::Wildcard);
    }

    if mt.ty != names::APPLICATION {
        return None;
    }

    // application/ld+json — subtype=ld, suffix=json
    if mt.subty == names::LD && mt.suffix == Some(names::JSON) {
        return Some(MatchedFormat::JsonLd);
    }

    // application/geo+json — subtype=geo, suffix=json
    if mt.subty == names::GEO && mt.suffix == Some(names::JSON) {
        return Some(MatchedFormat::GeoJson);
    }

    // application/json — exact match, no suffix (prevents application/merge-patch+json from matching)
    if mt.subty == names::JSON && mt.suffix.is_none() {
        return Some(MatchedFormat::Json);
    }

    None
}

/// Extract the `q` quality parameter value (defaults to 1.0 per RFC 7231).
fn quality_value(mt: &mediatype::MediaType) -> f32 {
    mt.get_param(names::Q).and_then(|v| v.as_str().parse::<f32>().ok()).unwrap_or(1.0)
}

fn parse_accept(parts: &Parts, allow_geo_json: bool) -> Result<ResponseFormat, ExtractorRejection> {
    let Some(accept_str) = parts.headers.get(ACCEPT).and_then(|v| v.to_str().ok()) else {
        // No Accept header → default to JSON
        return Ok(ResponseFormat::Json);
    };

    // Parse all entries with their quality values, keeping only supported ones
    let mut entries: Vec<AcceptEntry> = Vec::new();

    for result in MediaTypeList::new(accept_str) {
        let Ok(mt) = result else { continue };
        if let Some(format) = match_media_type(&mt) {
            // Skip geo+json if not allowed, but still consider it "recognized"
            // for the purposes of the 406 check below
            let q = quality_value(&mt);
            if matches!(format, MatchedFormat::GeoJson) && !allow_geo_json {
                continue;
            }
            entries.push(AcceptEntry { format, quality: q });
        }
    }

    if entries.is_empty() {
        // No supported type found → 406
        return Err(ExtractorRejection::Empty(axum::http::StatusCode::NOT_ACCEPTABLE));
    }

    // Sort by quality descending (stable sort preserves original order for equal q)
    entries.sort_by(|a, b| b.quality.partial_cmp(&a.quality).unwrap_or(std::cmp::Ordering::Equal));

    // Pick the highest-quality supported format
    let format = match entries[0].format {
        MatchedFormat::JsonLd => ResponseFormat::JsonLd,
        MatchedFormat::GeoJson => ResponseFormat::GeoJson,
        MatchedFormat::Json | MatchedFormat::Wildcard => ResponseFormat::Json,
    };

    Ok(format)
}

/// Extractor for Accept header content negotiation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptFormat(pub ResponseFormat);

impl AcceptFormat {
    pub fn format(&self) -> ResponseFormat {
        self.0
    }
}

/// Extractor variant for endpoints that do not support GeoJSON responses.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptFormatNoGeo(pub ResponseFormat);

impl AcceptFormatNoGeo {
    pub fn format(&self) -> ResponseFormat {
        self.0
    }
}

impl<S> FromRequestParts<S> for AcceptFormat
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parse_accept(parts, true).map(AcceptFormat)
    }
}

impl<S> FromRequestParts<S> for AcceptFormatNoGeo
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parse_accept(parts, false).map(AcceptFormatNoGeo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    // --- Unit tests for format matching ---

    fn parse_format(accept: &str, allow_geo: bool) -> Result<ResponseFormat, ()> {
        let req = Request::builder().uri("/test").header(ACCEPT, accept).body(()).unwrap();
        let (parts, _) = req.into_parts();
        parse_accept(&parts, allow_geo).map_err(|_| ())
    }

    #[test]
    fn exact_json() {
        assert_eq!(parse_format("application/json", true).unwrap(), ResponseFormat::Json);
    }

    #[test]
    fn exact_jsonld() {
        assert_eq!(parse_format("application/ld+json", true).unwrap(), ResponseFormat::JsonLd);
    }

    #[test]
    fn exact_geojson() {
        assert_eq!(parse_format("application/geo+json", true).unwrap(), ResponseFormat::GeoJson);
    }

    #[test]
    fn wildcard_defaults_to_json() {
        assert_eq!(parse_format("*/*", true).unwrap(), ResponseFormat::Json);
    }

    #[test]
    fn jsonld_with_params() {
        assert_eq!(parse_format("application/ld+json; charset=utf-8", true).unwrap(), ResponseFormat::JsonLd);
    }

    #[test]
    fn quality_prefers_higher() {
        // ld+json has q=0.9, json has q=1.0 (default) → json wins
        assert_eq!(parse_format("application/ld+json;q=0.9, application/json", true).unwrap(), ResponseFormat::Json);
    }

    #[test]
    fn quality_prefers_jsonld_when_higher() {
        assert_eq!(
            parse_format("application/json;q=0.5, application/ld+json;q=1.0", true).unwrap(),
            ResponseFormat::JsonLd
        );
    }

    #[test]
    fn json_patch_does_not_match_json() {
        // This was a false positive with .contains() — application/json-patch+json
        // should NOT match application/json
        assert!(parse_format("application/json-patch+json", true).is_err());
    }

    #[test]
    fn merge_patch_json_does_not_match_json() {
        // application/merge-patch+json should NOT match application/json
        assert!(parse_format("application/merge-patch+json", true).is_err());
    }

    #[test]
    fn rejects_xml() {
        assert!(parse_format("application/xml", true).is_err());
    }

    #[test]
    fn content_type_strings() {
        assert_eq!(ResponseFormat::Json.content_type(), "application/json");
        assert_eq!(ResponseFormat::JsonLd.content_type(), "application/ld+json");
        assert_eq!(ResponseFormat::GeoJson.content_type(), "application/geo+json");
    }

    #[test]
    fn include_context_in_body() {
        assert!(!ResponseFormat::Json.include_context_in_body());
        assert!(ResponseFormat::JsonLd.include_context_in_body());
        assert!(!ResponseFormat::GeoJson.include_context_in_body());
    }

    // --- Extractor integration tests ---

    #[tokio::test]
    async fn rejects_unsupported_accept_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(ACCEPT, "application/xml")
            .body(axum::body::Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                assert_eq!(status, axum::http::StatusCode::NOT_ACCEPTABLE);
            }
            _ => panic!("expected 406 empty rejection"),
        }
    }

    #[tokio::test]
    async fn no_geo_rejects_geojson_accept_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities")
            .header(ACCEPT, "application/geo+json")
            .body(axum::body::Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormatNoGeo::from_request_parts(&mut parts, &()).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                assert_eq!(status, axum::http::StatusCode::NOT_ACCEPTABLE);
            }
            _ => panic!("expected 406 empty rejection for geo+json"),
        }
    }

    #[tokio::test]
    async fn no_geo_accepts_jsonld() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities")
            .header(ACCEPT, "application/ld+json")
            .body(axum::body::Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormatNoGeo::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn no_accept_header_defaults_to_json() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(result.0, ResponseFormat::Json);
    }

    #[tokio::test]
    async fn multi_type_accept_picks_best() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(ACCEPT, "application/xml, application/ld+json;q=0.9, application/json")
            .body(axum::body::Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await.unwrap();
        // application/json has q=1.0 (default), application/ld+json has q=0.9
        assert_eq!(result.0, ResponseFormat::Json);
    }
}
