//! Language negotiation extractor for NGSI-LD requests.
//!
//! Resolves an optional [`LanguageFilter`] from the `NGSILD-Language` header.
//! Query parameters are parsed by the endpoint-specific request model so each
//! wire value is decoded exactly once.

use andromeda_http_headers::{language::LanguageFilter, ngsild_language::NgsiLdLanguage};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;

/// Axum extractor that resolves an optional [`LanguageFilter`] from the request.
///
/// If the header is absent, the inner value is `None`. An invalid header
/// produces a `BadRequestData` rejection.
pub struct LanguageNegotiation(pub Option<LanguageFilter>);

impl<S> FromRequestParts<S> for LanguageNegotiation
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();

        if parts.headers.contains_key("ngsild-language") {
            let header = parts
                .headers
                .typed_get::<NgsiLdLanguage>()
                .ok_or_else(|| ExtractorRejection::bad_request_data("Invalid NGSILD-Language header value", uri))?;
            return Ok(LanguageNegotiation(Some(header.into_filter())));
        }

        Ok(LanguageNegotiation(None))
    }
}

#[cfg(test)]
mod tests {
    use super::LanguageNegotiation;
    use axum::{extract::FromRequestParts, http::Request};

    async fn extract_from_uri(uri: &str) -> LanguageNegotiation {
        let (mut parts, _) = Request::builder().uri(uri).body(()).expect("valid request").into_parts();
        LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction")
    }

    async fn extract_with_header(header_value: &str) -> LanguageNegotiation {
        let (mut parts, _) = Request::builder()
            .uri("/entities")
            .header("NGSILD-Language", header_value)
            .body(())
            .expect("valid request")
            .into_parts();
        LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction")
    }

    #[tokio::test]
    async fn extracts_language_filter_from_header() {
        let LanguageNegotiation(filter) = extract_with_header("de, en;q=0.5").await;
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered.len(), 2);
    }

    #[tokio::test]
    async fn query_parameter_is_left_to_the_request_model() {
        let (mut parts, _) = Request::builder().uri("/entities?lang=fr").body(()).expect("valid request").into_parts();
        let LanguageNegotiation(filter) = LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction");
        assert!(filter.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_absent() {
        let LanguageNegotiation(filter) = extract_from_uri("/entities").await;
        assert!(filter.is_none());
    }

    #[tokio::test]
    async fn rejects_empty_header() {
        let (mut parts, _) = Request::builder()
            .uri("/entities")
            .header("NGSILD-Language", "")
            .body(())
            .expect("valid request")
            .into_parts();
        // Empty header is present but invalid — must reject with BadRequestData
        let result = LanguageNegotiation::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }
}
