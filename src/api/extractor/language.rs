use crate::{api::extractor::error::ExtractorRejection, query::lang::LanguageFilter};
use axum::{extract::FromRequestParts, http::request::Parts};
use url::form_urlencoded;

/// Custom HTTP header for language negotiation.
///
/// Acts as a fallback when the `lang` query parameter is absent. The query
/// parameter always takes precedence per NGSI-LD convention.
pub const NGSILD_LANGUAGE_HEADER: &str = "NGSILD-Language";

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

        // 2. Try NGSILD-Language header as fallback
        let lang_from_header = if lang_from_query.is_none() {
            match parts.headers.get(NGSILD_LANGUAGE_HEADER) {
                Some(value) => {
                    let s = value
                        .to_str()
                        .map_err(|_| ExtractorRejection::bad_request_data("Invalid NGSILD-Language header encoding", uri.clone()))?
                        .trim();
                    if s.is_empty() {
                        return Err(ExtractorRejection::bad_request_data("NGSILD-Language header must not be empty", uri));
                    }
                    Some(s.to_string())
                }
                None => None,
            }
        } else {
            None
        };

        let raw = lang_from_query.or(lang_from_header);

        match raw {
            Some(value) => {
                let filter = LanguageFilter::parse(&value).map_err(|error| ExtractorRejection::bad_request_data(error.to_string(), uri))?;
                Ok(LanguageNegotiation(Some(filter)))
            }
            None => Ok(LanguageNegotiation(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LanguageNegotiation, NGSILD_LANGUAGE_HEADER};
    use axum::{extract::FromRequestParts, http::Request};

    async fn extract_from_uri(uri: &str) -> LanguageNegotiation {
        let (mut parts, _) = Request::builder().uri(uri).body(()).unwrap().into_parts();
        LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction")
    }

    async fn extract_with_header(header_value: &str) -> LanguageNegotiation {
        let (mut parts, _) = Request::builder()
            .uri("/entities")
            .header(NGSILD_LANGUAGE_HEADER, header_value)
            .body(())
            .unwrap()
            .into_parts();
        LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction")
    }

    #[tokio::test]
    async fn extracts_language_filter_from_query_parameter() {
        let LanguageNegotiation(filter) = extract_from_uri("/entities?lang=fr,en;q=0.8").await;
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 2);
    }

    #[tokio::test]
    async fn extracts_language_filter_from_header() {
        let LanguageNegotiation(filter) = extract_with_header("de,en;q=0.5").await;
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 2);
    }

    #[tokio::test]
    async fn query_parameter_takes_precedence_over_header() {
        let (mut parts, _) = Request::builder()
            .uri("/entities?lang=fr")
            .header(NGSILD_LANGUAGE_HEADER, "de")
            .body(())
            .unwrap()
            .into_parts();
        let LanguageNegotiation(filter) = LanguageNegotiation::from_request_parts(&mut parts, &()).await.expect("extraction");
        let filter = filter.expect("language filter present");
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 1);
        assert!(matches!(
            ordered[0].range(),
            crate::query::lang::LanguageRange::Tag(tag) if tag.to_string() == "fr"
        ));
    }

    #[tokio::test]
    async fn returns_none_when_absent() {
        let LanguageNegotiation(filter) = extract_from_uri("/entities").await;
        assert!(filter.is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_language_filter() {
        let (mut parts, _) = Request::builder().uri("/entities?lang=").body(()).unwrap().into_parts();
        let result = LanguageNegotiation::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_empty_header() {
        let (mut parts, _) = Request::builder()
            .uri("/entities")
            .header(NGSILD_LANGUAGE_HEADER, "")
            .body(())
            .unwrap()
            .into_parts();
        let result = LanguageNegotiation::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }
}
