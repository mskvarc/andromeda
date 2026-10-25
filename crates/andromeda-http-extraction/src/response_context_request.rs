//! What a request that renders a body fixes about the `@context` its response
//! names.
//!
//! A response names the context of the request it answers (clauses 5.2.3, 4.4
//! and 5.5.7), and a compound one through a wrapper hosted under the request's
//! `apiRoot` (clause 6.2, Annex C.8). A reading operation takes both from its
//! request together: the context from the one `Link` header clause 6.3.5
//! allows ([`ContextUrl`]), and the `apiRoot` the wrapper's URL is built on
//! ([`ApiRoot`]). An operation that renders nothing extracts neither.

use crate::{api_root::ApiRoot, context::ContextUrl};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::context_spec::ContextSpec;
use axum::{extract::FromRequestParts, http::request::Parts};

/// The request context a reading operation's response names, and the `apiRoot`
/// a hosted wrapper naming it is served under.
pub struct ResponseContextRequest {
    /// The context the request named in its `Link` header (clause 6.3.5), or
    /// the core context alone.
    pub context: ContextSpec,
    /// The `apiRoot` the request is answered under (clause 6.2).
    pub api_root: ApiRoot,
}

impl<S> FromRequestParts<S> for ResponseContextRequest
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<ResponseContextRequest, ExtractorRejection> {
        let ContextUrl(context) = ContextUrl::from_request_parts(parts, state).await?;
        let api_root = ApiRoot::from_request_parts(parts, state).await?;
        Ok(ResponseContextRequest { context, api_root })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::response_context_request::ResponseContextRequest;
    use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
    use axum::{extract::FromRequestParts, http::Request};

    #[tokio::test]
    async fn a_request_without_a_link_names_the_core_context_under_its_own_origin() {
        let (mut parts, ()) = Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1")
            .header("host", "broker.example")
            .body(())
            .expect("a valid request")
            .into_parts();

        let Ok(ResponseContextRequest { context, api_root }) = ResponseContextRequest::from_request_parts(&mut parts, &()).await else {
            panic!("the request context is read");
        };

        pretty_assertions::assert_eq!(context, ContextSpec::from_request(RequestContext::Default));
        pretty_assertions::assert_eq!(api_root.url().map(|url| url.as_str().to_owned()), Some("http://broker.example/".to_owned()));
    }

    #[tokio::test]
    async fn an_authority_that_forms_no_origin_is_refused() {
        let (mut parts, ()) = Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Entity:1")
            .header("host", "broker.example:99999")
            .body(())
            .expect("a valid request")
            .into_parts();

        assert!(ResponseContextRequest::from_request_parts(&mut parts, &()).await.is_err());
    }
}
