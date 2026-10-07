//! Accept header extraction for NGSI-LD content negotiation.
//!
//! Uses the typed `Accept` header from `crate::http::header` for structured media type
//! parsing, preventing false positives like `application/json-patch+json`
//! matching `application/json`.

use andromeda_http_headers::{
    accept::Accept,
    media_type::{GeoJsonSupport, ResponseMediaType},
};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, not_acceptable::AvailableRepresentations};
use axum::{
    extract::FromRequestParts,
    http::{header::ACCEPT, request::Parts},
};
use headers::HeaderMapExt;
use std::future::{Future, ready};

/// Parse the Accept header and select the best media type.
///
/// Clause 6.3.4: a refusal is "a 406 HTTP status code and the body of the
/// message shall contain the list of the available representations of the
/// resources", so the rejection carries the endpoint's own list rather than a
/// bare status.
fn resolve_accept(parts: &Parts, geo_json: GeoJsonSupport) -> Result<ResponseMediaType, ExtractorRejection> {
    // Clause 6.3.4: "If the Accept header is not present, application/json
    // shall be assumed."
    if !parts.headers.contains_key(ACCEPT) {
        return Ok(ResponseMediaType::Json);
    }

    let not_acceptable = || ExtractorRejection::NotAcceptable(AvailableRepresentations::of(geo_json));

    // Header present but could not be decoded (no recognized NGSI-LD media types) -> 406
    let Some(accept) = parts.headers.typed_get::<Accept>() else {
        return Err(not_acceptable());
    };

    accept.best_format(geo_json).ok_or_else(not_acceptable)
}

/// Generate an Accept-header extractor struct with a `media_type()` accessor
/// and a `FromRequestParts` impl that delegates to [`resolve_accept`].
macro_rules! impl_accept_extractor {
    (
        $(#[$meta:meta])*
        $name:ident, $geo:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name(pub ResponseMediaType);

        impl $name {
            /// The negotiated response media type.
            pub const fn media_type(&self) -> ResponseMediaType {
                self.0
            }
        }

        impl<S> FromRequestParts<S> for $name
        where
            S: Send + Sync,
        {
            type Rejection = ExtractorRejection;

            fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
                ready(resolve_accept(parts, $geo).map($name))
            }
        }
    };
}

impl_accept_extractor!(
    /// Extractor for Accept header content negotiation (`GeoJSON` allowed).
    AcceptFormat,
    GeoJsonSupport::Allowed
);

impl_accept_extractor!(
    /// Extractor variant for endpoints that do not support `GeoJSON` responses.
    AcceptFormatNoGeo,
    GeoJsonSupport::Disallowed
);

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, header::ACCEPT},
    };

    fn parse_format(accept: &str, geo_json: GeoJsonSupport) -> Result<ResponseMediaType, ()> {
        let req = Request::builder().uri("/test").header(ACCEPT, accept).body(()).expect("valid request");
        let (parts, ()) = req.into_parts();
        resolve_accept(&parts, geo_json).map_err(|_| ())
    }

    #[test]
    fn exact_json() {
        pretty_assertions::assert_eq!(parse_format("application/json", GeoJsonSupport::Allowed), Ok(ResponseMediaType::Json));
    }

    #[test]
    fn exact_jsonld() {
        pretty_assertions::assert_eq!(parse_format("application/ld+json", GeoJsonSupport::Allowed), Ok(ResponseMediaType::JsonLd));
    }

    #[test]
    fn exact_geojson() {
        pretty_assertions::assert_eq!(parse_format("application/geo+json", GeoJsonSupport::Allowed), Ok(ResponseMediaType::GeoJson));
    }

    #[test]
    fn wildcard_defaults_to_json() {
        pretty_assertions::assert_eq!(parse_format("*/*", GeoJsonSupport::Allowed), Ok(ResponseMediaType::Json));
    }

    #[test]
    fn jsonld_with_params() {
        pretty_assertions::assert_eq!(
            parse_format("application/ld+json; charset=utf-8", GeoJsonSupport::Allowed),
            Ok(ResponseMediaType::JsonLd)
        );
    }

    #[test]
    fn quality_prefers_higher() {
        // ld+json has q=0.9, json has q=1.0 (default) -> json wins
        pretty_assertions::assert_eq!(
            parse_format("application/ld+json;q=0.9, application/json", GeoJsonSupport::Allowed),
            Ok(ResponseMediaType::Json)
        );
    }

    #[test]
    fn quality_prefers_jsonld_when_higher() {
        pretty_assertions::assert_eq!(
            parse_format("application/json;q=0.5, application/ld+json;q=1.0", GeoJsonSupport::Allowed),
            Ok(ResponseMediaType::JsonLd)
        );
    }

    #[test]
    fn json_patch_does_not_match_json() {
        assert!(parse_format("application/json-patch+json", GeoJsonSupport::Allowed).is_err());
    }

    #[test]
    fn merge_patch_json_does_not_match_json() {
        assert!(parse_format("application/merge-patch+json", GeoJsonSupport::Allowed).is_err());
    }

    #[test]
    fn rejects_xml() {
        assert!(parse_format("application/xml", GeoJsonSupport::Allowed).is_err());
    }

    /// The representations a rejection published, or a description of what it
    /// was instead.
    fn rejected_representations(rejection: ExtractorRejection) -> Vec<&'static str> {
        match rejection {
            ExtractorRejection::NotAcceptable(representations) => representations.media_types().iter().map(ResponseMediaType::as_str).collect(),
            ExtractorRejection::Empty(status) => panic!("clause 6.3.4 requires a representation list, not a bare {status}"),
            ExtractorRejection::InvalidRequest { detail, .. } | ExtractorRejection::BadRequestData { detail, .. } => {
                panic!("clause 6.3.4 requires a 406, not a problem detail: {detail}")
            }
            ExtractorRejection::Classified(_) => panic!("clause 6.3.4 requires a 406 with a representation list"),
        }
    }

    /// Clause 6.3.4: an `Accept` that "does not imply application/json nor
    /// application/ld+json" is refused with the list of representations the
    /// endpoint does offer, `GeoJSON` included where the endpoint answers it.
    #[tokio::test]
    async fn rejects_unsupported_accept_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(ACCEPT, "application/xml")
            .body(Body::empty())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let rejection = AcceptFormat::from_request_parts(&mut parts, &()).await.expect_err("application/xml is refused");
        pretty_assertions::assert_eq!(
            rejected_representations(rejection),
            vec!["application/json", "application/ld+json", "application/geo+json"]
        );
    }

    /// Clause 6.3.4: for an operation outside Retrieve Entity and Query Entity,
    /// `application/geo+json` "will result in a Not Acceptable Media Type
    /// error", and the list it publishes must not offer the media type it just
    /// refused.
    #[tokio::test]
    async fn no_geo_rejects_geojson_accept_header() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities")
            .header(ACCEPT, "application/geo+json")
            .body(Body::empty())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let rejection = AcceptFormatNoGeo::from_request_parts(&mut parts, &())
            .await
            .expect_err("application/geo+json is refused here");
        pretty_assertions::assert_eq!(rejected_representations(rejection), vec!["application/json", "application/ld+json"]);
    }

    #[tokio::test]
    async fn no_geo_accepts_jsonld() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities")
            .header(ACCEPT, "application/ld+json")
            .body(Body::empty())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormatNoGeo::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn no_accept_header_defaults_to_json() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(Body::empty()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await.expect("should succeed");
        pretty_assertions::assert_eq!(result.0, ResponseMediaType::Json);
    }

    #[tokio::test]
    async fn multi_type_accept_picks_best() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(ACCEPT, "application/xml, application/ld+json;q=0.9, application/json")
            .body(Body::empty())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = AcceptFormat::from_request_parts(&mut parts, &()).await.expect("should succeed");
        // application/json has q=1.0 (default), application/ld+json has q=0.9
        pretty_assertions::assert_eq!(result.0, ResponseMediaType::Json);
    }
}
