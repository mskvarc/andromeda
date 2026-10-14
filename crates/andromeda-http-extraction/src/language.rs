//! Language negotiation extractor for NGSI-LD requests.
//!
//! Resolves an optional [`LanguageFilter`] from the `NGSILD-Language` header,
//! or, failing that, from the `Accept-Language` header the
//! [`crate::accept_language`] layer accepted. Query parameters are parsed by
//! the endpoint-specific request model so each wire value is decoded exactly
//! once, and the handler lets a `lang` outrank either header.

use crate::{accept_language::AcceptedLanguage, addressed_uri::addressed_uri};
use andromeda_http_headers::{language::LanguageFilter, ngsild_language::NgsiLdLanguage};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use std::future::{Future, ready};

/// Axum extractor that resolves an optional [`LanguageFilter`] from the request.
///
/// If neither header names a language, the inner value is `None`. An invalid
/// `NGSILD-Language` produces a `BadRequestData` rejection.
pub struct LanguageNegotiation(pub Option<LanguageFilter>);

/// Decode the `NGSILD-Language` header, rejecting a header that is present but
/// not a valid language list, and fall back to the accepted `Accept-Language`.
fn resolve_language_negotiation(parts: &mut Parts) -> Result<LanguageNegotiation, ExtractorRejection> {
    if parts.headers.contains_key("ngsild-language") {
        let header = parts
            .headers
            .typed_get::<NgsiLdLanguage>()
            .ok_or_else(|| ExtractorRejection::bad_request_data("Invalid NGSILD-Language header value", addressed_uri(parts)))?;
        return Ok(LanguageNegotiation(Some(header.into_filter())));
    }

    Ok(LanguageNegotiation(
        parts.extensions.remove::<AcceptedLanguage>().map(|AcceptedLanguage(filter)| filter),
    ))
}

impl<S> FromRequestParts<S> for LanguageNegotiation
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_language_negotiation(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::LanguageNegotiation;
    use axum::{extract::FromRequestParts, http::Request};

    async fn extract_from_uri(uri: &str) -> LanguageNegotiation {
        let (mut parts, ()) = Request::builder().uri(uri).body(()).expect("valid request").into_parts();
        LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction")
    }

    async fn extract_with_header(header_value: &str) -> LanguageNegotiation {
        let (mut parts, ()) = Request::builder()
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
        let (mut parts, ()) = Request::builder().uri("/entities?lang=fr").body(()).expect("valid request").into_parts();
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
        let (mut parts, ()) = Request::builder()
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
