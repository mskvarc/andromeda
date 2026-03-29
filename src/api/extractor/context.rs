//! JSON-LD context URL extraction from Link headers.

use axum::{extract::FromRequestParts, http::request::Parts};
use json_ld_engine::ContextSpec;

/// JSON-LD context extracted from Link header.
#[derive(Clone, Debug, Default)]
pub struct ContextUrl(pub ContextSpec);

impl ContextUrl {
    pub fn into_inner(self) -> ContextSpec {
        self.0
    }

    /// Merge with another context spec (e.g. from body).
    pub fn merge(self, other: ContextSpec) -> ContextSpec {
        let mut spec = self.0;
        if let Some(body_ctx) = other.body_context() {
            spec = spec.with_body_context(Some(body_ctx.clone()));
        }
        spec
    }
}

impl<S> FromRequestParts<S> for ContextUrl
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let mut links = Vec::new();
        for value in parts.headers.get_all("Link") {
            let Ok(link_str) = value.to_str() else {
                continue;
            };
            for segment in link_str.split(',') {
                if let Ok(map) = parse_link_header::parse(segment) {
                    for (rel, link) in map {
                        if let Some(rel) = rel {
                            if rel.split_whitespace().any(|r| r == "http://www.w3.org/ns/json-ld#context") {
                                links.push(link.raw_uri.clone());
                            }
                        }
                    }
                }
            }
        }

        // Keep request-provided Link contexts only.
        // Core fallback is applied later by context_resolver when no explicit
        // context exists from Link/document/body.
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

    #[test]
    fn test_parse_with_valid_context_link() {
        let link = r#"<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
        let result = parse_link_header::parse_with_rel(link).unwrap();
        let context_link = result.get("http://www.w3.org/ns/json-ld#context").unwrap();
        assert_eq!(context_link.raw_uri, "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld");
    }

    #[test]
    fn test_parse_with_wrong_rel() {
        let link = r#"<https://example.com/other>; rel="stylesheet""#;
        let result = parse_link_header::parse_with_rel(link).unwrap();
        assert!(result.get("http://www.w3.org/ns/json-ld#context").is_none());
    }

    #[tokio::test]
    async fn extracts_multiple_context_links() {
        let header = r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context", <https://example.com/b.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
        let request = axum::http::Request::builder().uri("/").header("Link", header).body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        let context = ContextUrl::from_request_parts(&mut parts, &()).await.unwrap().into_inner();

        assert_eq!(
            context.link_contexts(),
            &["https://example.com/a.jsonld".to_string(), "https://example.com/b.jsonld".to_string()]
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
