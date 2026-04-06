//! Accept header extraction for NGSI-LD content negotiation.
//!
//! Uses the typed `Accept` header from `crate::http::header` for structured media type
//! parsing, preventing false positives like `application/json-patch+json`
//! matching `application/json`.

use crate::{
    api::extractor::error::ExtractorRejection,
    http::{header::Accept, model::response_format::ResponseFormat},
};
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, header::ACCEPT, request::Parts},
};
use headers::HeaderMapExt;

/// Extractor for Accept header content negotiation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptFormat(pub ResponseFormat);

impl AcceptFormat {
    /// Return the negotiated response format.
    pub const fn format(&self) -> ResponseFormat {
        self.0
    }
}

/// Extractor variant for endpoints that do not support GeoJSON responses.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptFormatNoGeo(pub ResponseFormat);

impl AcceptFormatNoGeo {
    /// Return the negotiated response format.
    pub const fn format(&self) -> ResponseFormat {
        self.0
    }
}

/// Parse the Accept header and select the best format.
fn resolve_accept(parts: &Parts, allow_geo_json: bool) -> Result<ResponseFormat, ExtractorRejection> {
    // No Accept header at all -> default to JSON per NGSI-LD convention
    if !parts.headers.contains_key(ACCEPT) {
        return Ok(ResponseFormat::Json);
    }

    // Header present but could not be decoded (no recognized NGSI-LD media types) -> 406
    let Some(accept) = parts.headers.typed_get::<Accept>() else {
        return Err(ExtractorRejection::Empty(StatusCode::NOT_ACCEPTABLE));
    };

    accept.best_format(allow_geo_json).ok_or(ExtractorRejection::Empty(StatusCode::NOT_ACCEPTABLE))
}

impl<S> FromRequestParts<S> for AcceptFormat
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        resolve_accept(parts, true).map(AcceptFormat)
    }
}

impl<S> FromRequestParts<S> for AcceptFormatNoGeo
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        resolve_accept(parts, false).map(AcceptFormatNoGeo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, header::ACCEPT};

    // --- Unit tests for format matching ---

    fn parse_format(accept: &str, allow_geo: bool) -> Result<ResponseFormat, ()> {
        let req = Request::builder().uri("/test").header(ACCEPT, accept).body(()).expect("valid request");
        let (parts, _) = req.into_parts();
        resolve_accept(&parts, allow_geo).map_err(|_| ())
    }

    #[test]
    fn exact_json() {
        assert_eq!(parse_format("application/json", true), Ok(ResponseFormat::Json));
    }

    #[test]
    fn exact_jsonld() {
        assert_eq!(parse_format("application/ld+json", true), Ok(ResponseFormat::JsonLd));
    }

    #[test]
    fn exact_geojson() {
        assert_eq!(parse_format("application/geo+json", true), Ok(ResponseFormat::GeoJson));
    }

    #[test]
    fn wildcard_defaults_to_json() {
        assert_eq!(parse_format("*/*", true), Ok(ResponseFormat::Json));
    }

    #[test]
    fn jsonld_with_params() {
        assert_eq!(parse_format("application/ld+json; charset=utf-8", true), Ok(ResponseFormat::JsonLd));
    }

    #[test]
    fn quality_prefers_higher() {
        // ld+json has q=0.9, json has q=1.0 (default) -> json wins
        assert_eq!(parse_format("application/ld+json;q=0.9, application/json", true), Ok(ResponseFormat::Json));
    }

    #[test]
    fn quality_prefers_jsonld_when_higher() {
        assert_eq!(
            parse_format("application/json;q=0.5, application/ld+json;q=1.0", true),
            Ok(ResponseFormat::JsonLd)
        );
    }

    #[test]
    fn json_patch_does_not_match_json() {
        assert!(parse_format("application/json-patch+json", true).is_err());
    }

    #[test]
    fn merge_patch_json_does_not_match_json() {
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
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
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
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormatNoGeo::from_request_parts(&mut parts, &()).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
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
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormatNoGeo::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn no_accept_header_defaults_to_json() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .body(axum::body::Body::empty())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await.expect("should succeed");
        assert_eq!(result.0, ResponseFormat::Json);
    }

    #[tokio::test]
    async fn multi_type_accept_picks_best() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(ACCEPT, "application/xml, application/ld+json;q=0.9, application/json")
            .body(axum::body::Body::empty())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await.expect("should succeed");
        // application/json has q=1.0 (default), application/ld+json has q=0.9
        assert_eq!(result.0, ResponseFormat::Json);
    }
}
