//! Language negotiation extractor for NGSI-LD requests.
//!
//! Resolves an optional [`LanguageFilter`] from the request using two sources
//! (in priority order):
//! 1. The `lang` query parameter (spec-mandated, NGSI-LD clause 6.3.5)
//! 2. The `NGSILD-Language` custom header (Andromeda-specific fallback)

use crate::http::extractor::error::ExtractorRejection;
use andromeda_ngsi_ld_http_headers::{language::LanguageFilter, ngsild_language::NgsiLdLanguage};
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use url::form_urlencoded;

/// Axum extractor that resolves an optional [`LanguageFilter`] from the request.
///
/// Extraction sources (in priority order):
/// 1. The `lang` query parameter (spec-mandated, NGSI-LD 6.3.5)
/// 2. The `NGSILD-Language` custom header (fallback)
///
/// If neither source is present, the inner value is `None`. Invalid values in
/// either source produce a `BadRequestData` rejection.
pub struct LanguageNegotiation(pub Option<LanguageFilter>);

impl<S> FromRequestParts<S> for LanguageNegotiation
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();

        // 1. Try `lang` query parameter (takes precedence)
        let lang_from_query = parts.uri.query().and_then(|qs| {
            form_urlencoded::parse(qs.as_bytes())
                .find(|(key, _)| key == "lang")
                .map(|(_, value)| value.into_owned())
        });

        if let Some(value) = lang_from_query {
            let filter = LanguageFilter::parse(&value).map_err(|error| ExtractorRejection::bad_request_data(error.to_string(), uri))?;
            return Ok(LanguageNegotiation(Some(filter)));
        }

        // 2. Try NGSILD-Language header as fallback.
        //    If the header is present but malformed, reject with 400 rather than
        //    silently ignoring it (`typed_get` returns `None` on decode failure).
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
    async fn extracts_language_filter_from_query_parameter() {
        let LanguageNegotiation(filter) = extract_from_uri("/entities?lang=fr,en;q=0.8").await;
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered.len(), 2);
    }

    #[tokio::test]
    async fn extracts_language_filter_from_header() {
        let LanguageNegotiation(filter) = extract_with_header("de, en;q=0.5").await;
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered.len(), 2);
    }

    #[tokio::test]
    async fn query_parameter_takes_precedence_over_header() {
        let (mut parts, _) = Request::builder()
            .uri("/entities?lang=fr")
            .header("NGSILD-Language", "de")
            .body(())
            .expect("valid request")
            .into_parts();
        let LanguageNegotiation(filter) = LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction");
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        pretty_assertions::assert_eq!(ordered.len(), 1);
        assert!(matches!(
            ordered[0].value(),
            andromeda_ngsi_ld_http_headers::language::LanguageRange::Tag(tag) if tag == "fr"
        ));
    }

    #[tokio::test]
    async fn returns_none_when_absent() {
        let LanguageNegotiation(filter) = extract_from_uri("/entities").await;
        assert!(filter.is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_language_filter() {
        let (mut parts, _) = Request::builder().uri("/entities?lang=").body(()).expect("valid request").into_parts();
        let result = LanguageNegotiation::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
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
