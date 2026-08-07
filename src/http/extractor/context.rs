//! JSON-LD context URL extraction from Link headers.

use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_http_headers::link::header::Link;
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

impl<S> FromRequestParts<S> for ContextUrl
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let mut links = Vec::new();
        for value in parts.headers.get_all(LINK) {
            let Ok(link_str) = value.to_str() else {
                continue;
            };
            if let Ok(parsed) = Link::from_str(link_str) {
                links.extend(parsed.context_urls().cloned());
            }
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
        let request = axum::http::Request::builder().uri("/").header("Link", header).body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        let context = ContextUrl::from_request_parts(&mut parts, &()).await.unwrap().into_inner();

        pretty_assertions::assert_eq!(
            context.link_contexts(),
            &[iri_ref("https://example.com/a.jsonld"), iri_ref("https://example.com/b.jsonld")]
        );
    }

    #[tokio::test]
    async fn no_link_header_keeps_context_spec_empty() {
        let request = axum::http::Request::builder().uri("/").body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        let context = ContextUrl::from_request_parts(&mut parts, &()).await.unwrap().into_inner();

        assert!(context.link_contexts().is_empty());
        assert!(context.body_context().is_none());
    }
}
