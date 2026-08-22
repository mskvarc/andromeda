//! JSON-LD context URL extraction from Link headers.

use andromeda_http_headers::link::header::Link;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::context_spec::ContextSpec;
use axum::{
    extract::FromRequestParts,
    http::{header::LINK, request::Parts},
};
use std::str::FromStr;

/// JSON-LD context extracted from Link header.
#[derive(Clone, Debug, Default)]
pub struct ContextUrl(pub ContextSpec);

impl ContextUrl {
    /// Consume the extractor and return the inner [`ContextSpec`].
    pub fn into_inner(self) -> ContextSpec {
        self.0
    }

    /// Merge with another context spec (e.g. from body).
    pub fn merge(self, other: ContextSpec) -> ContextSpec {
        self.0.with_body_context(other.into_body_context())
    }
}

/// The clause 5.5.4 JSON-LD validation of a request's `Link` headers: a
/// header that is not a well-formed `Link` (IETF RFC 8288) cannot name a
/// context, and the request is rejected as `BadRequestData` rather than served
/// as if it had named none.
impl<S> FromRequestParts<S> for ContextUrl
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let mut links = Vec::new();
        for value in parts.headers.get_all(LINK) {
            let link_str = value
                .to_str()
                .map_err(|error| ExtractorRejection::bad_request_data(format!("Link header is not visible ASCII: {error}"), parts.uri.clone()))?;
            let parsed = Link::from_str(link_str)
                .map_err(|error| ExtractorRejection::bad_request_data(format!("Link header is malformed: {error}"), parts.uri.clone()))?;
            links.extend(parsed.context_urls().cloned());
        }

        let context = if links.is_empty() {
            ContextSpec::none()
        } else {
            ContextSpec::from_links(links)
        };

        Ok(ContextUrl(context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use iri_rs::IriRefBuf;

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    #[test]
    fn parse_with_valid_context_link() {
        let raw = r#"<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
        let link = Link::from_str(raw).expect("should parse");
        let values = link.values();
        pretty_assertions::assert_eq!(values.len(), 1);
        pretty_assertions::assert_eq!(values[0].link(), "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld");
        assert!(values[0].is_context_link());
    }

    #[test]
    fn parse_with_wrong_rel() {
        let raw = r#"<https://example.com/other>; rel="stylesheet""#;
        let link = Link::from_str(raw).expect("should parse");
        let values = link.values();
        pretty_assertions::assert_eq!(values.len(), 1);
        assert!(!values[0].is_context_link());
    }

    #[tokio::test]
    async fn extracts_multiple_context_links() {
        let header = r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context", <https://example.com/b.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
        let request = Request::builder().uri("/").header("Link", header).body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        let context = ContextUrl::from_request_parts(&mut parts, &()).await.unwrap().into_inner();

        pretty_assertions::assert_eq!(
            context.link_contexts(),
            &[iri_ref("https://example.com/a.jsonld"), iri_ref("https://example.com/b.jsonld")]
        );
    }

    /// Clause 5.5.4: a `Link` header that is not a well-formed link is a
    /// `BadRequestData`, not an absent context.
    #[tokio::test]
    async fn a_malformed_link_header_is_rejected_as_bad_request_data() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/csourceRegistrations?type=Vehicle")
            .header("Link", "this is not a valid link header")
            .body(())
            .unwrap();
        let (mut parts, _) = request.into_parts();

        let rejection = ContextUrl::from_request_parts(&mut parts, &()).await.expect_err("malformed Link is rejected");

        assert!(matches!(rejection, ExtractorRejection::BadRequestData { .. }), "{rejection:?}");
    }

    /// A well-formed `Link` naming a relation other than the JSON-LD context
    /// is not a context, but it is not an error either.
    #[tokio::test]
    async fn a_link_with_another_relation_leaves_the_context_empty() {
        let request = Request::builder()
            .uri("/")
            .header("Link", r#"<https://example.com/other>; rel="stylesheet""#)
            .body(())
            .unwrap();
        let (mut parts, _) = request.into_parts();

        let context = ContextUrl::from_request_parts(&mut parts, &()).await.unwrap().into_inner();

        assert!(context.link_contexts().is_empty());
    }

    #[tokio::test]
    async fn no_link_header_keeps_context_spec_empty() {
        let request = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        let context = ContextUrl::from_request_parts(&mut parts, &()).await.unwrap().into_inner();

        assert!(context.link_contexts().is_empty());
        assert!(context.body_context().is_none());
    }
}
