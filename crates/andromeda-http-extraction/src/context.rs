//! The JSON-LD context a request without an `application/ld+json` body names.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.5 reads the context of a request without
//! a body, or with an `application/json` one, "from a Link header [7] as
//! mandated by JSON-LD [2]". [`ContextUrl`] is that context, taken from the one
//! reading of the request's `Link` headers that [`crate::request_links`]
//! keeps.

use crate::{
    addressed_uri::addressed_uri,
    classification::classify_context_link_error,
    request_links::{PayloadCarrier, RequestLinks},
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// The JSON-LD context a request's `Link` headers named: one context IRI, or
/// the default context when the request named none.
#[derive(Clone, Debug, Default)]
pub struct ContextUrl(pub ContextSpec);

impl ContextUrl {
    /// Consume the extractor and return the inner [`ContextSpec`].
    #[must_use]
    pub fn into_inner(self) -> ContextSpec {
        self.0
    }
}

/// The clause 5.5.4 JSON-LD validation of a request's `Link` headers, as the
/// [`ContextSpec`] a request without an `application/ld+json` body is read
/// against.
fn context_from_link_headers(parts: &mut Parts) -> Result<ContextSpec, ExtractorRejection> {
    let link = RequestLinks::of(parts)
        .and_then(|links| links.context_for(PayloadCarrier::LinkHeader))
        .map_err(|error| ExtractorRejection::Classified(classify_context_link_error(&error).with_instance(addressed_uri(parts))))?;
    Ok(ContextSpec::from_request(link.map_or(RequestContext::Default, RequestContext::Link)))
}

impl<S> FromRequestParts<S> for ContextUrl
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(context_from_link_headers(parts).map(ContextUrl))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::context::ContextUrl;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use andromeda_jsonld::request_context::RequestContext;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{extract::FromRequestParts, http::Request};
    use iri_rs::IriBuf;

    const CONTEXT_A: &str = r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
    const CONTEXT_B: &str = r#"<https://example.com/b.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
    const PROFILE: &str = r#"<https://uri.etsi.org/ngsi-ld/v1.9>; rel="profile""#;

    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    /// The context the body-less extractor reads from `link_lines`.
    async fn extracted(link_lines: &[&str]) -> Result<RequestContext, ExtractorRejection> {
        let mut request = Request::builder().uri("/ngsi-ld/v1/entities?type=T").header("host", "broker.example");
        for line in link_lines {
            request = request.header("Link", *line);
        }
        let mut parts = request.body(()).expect("a valid request").into_parts().0;
        ContextUrl::from_request_parts(&mut parts, &())
            .await
            .map(|context| context.into_inner().request_context().clone()) // clone: the spec is dropped here
    }

    /// Whether `rejection` is the clause 5.5.4 `BadRequestData`.
    fn is_bad_request_data(rejection: &ExtractorRejection) -> bool {
        matches!(rejection, ExtractorRejection::Classified(response) if response.problem().error_type() == NgsiLdErrorType::BadRequestData)
    }

    #[tokio::test]
    async fn no_link_header_is_the_default_context() {
        pretty_assertions::assert_eq!(extracted(&[]).await.expect("no Link is admitted"), RequestContext::Default);
    }

    #[tokio::test]
    async fn one_context_link_is_the_request_context() {
        pretty_assertions::assert_eq!(
            extracted(&[CONTEXT_A]).await.expect("one context link"),
            RequestContext::Link(iri("https://example.com/a.jsonld"))
        );
    }

    /// Clause 6.3.5 and JSON-LD 1.1 API section 9.4.1: two context links are
    /// refused as `BadRequestData`.
    #[tokio::test]
    async fn two_context_links_are_refused_as_bad_request_data() {
        let rejection = extracted(&[CONTEXT_A, CONTEXT_B]).await.expect_err("two context links are refused");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// A `profile` link (clause 6.3.21) and a pagination link are not
    /// contexts: they neither count nor refuse the request.
    #[tokio::test]
    async fn one_context_link_beside_links_of_other_relations_is_the_request_context() {
        pretty_assertions::assert_eq!(
            extracted(&[CONTEXT_A, PROFILE, r#"<https://example.com/next>; rel="next""#])
                .await
                .expect("one context link"),
            RequestContext::Link(iri("https://example.com/a.jsonld"))
        );
    }

    /// IETF RFC 8288 section 2.1.2: the relation is matched case-insensitively,
    /// so a capitalised one is still the context, and still counts.
    #[tokio::test]
    async fn a_context_relation_in_capitals_is_the_context_and_counts() {
        let capitalised = r#"<https://example.com/c.jsonld>; rel="HTTP://WWW.W3.ORG/ns/json-ld#context""#;
        pretty_assertions::assert_eq!(
            extracted(&[capitalised]).await.expect("the capitalised relation is a context"),
            RequestContext::Link(iri("https://example.com/c.jsonld"))
        );
        let rejection = extracted(&[capitalised, CONTEXT_A]).await.expect_err("it counts toward the limit");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// Clause 5.5.4: a `Link` header that is not a well-formed link is a
    /// `BadRequestData`, not an absent context.
    #[tokio::test]
    async fn a_malformed_link_header_is_rejected_as_bad_request_data() {
        let rejection = extracted(&["this is not a valid link header"]).await.expect_err("malformed Link is rejected");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// A single core context link, of any release, is admitted as the
    /// request's context (clause 4.4 then processes it as v1.9).
    #[tokio::test]
    async fn a_single_core_context_link_is_admitted() {
        let core = r#"<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
        pretty_assertions::assert_eq!(
            extracted(&[core]).await.expect("a core context link"),
            RequestContext::Link(iri("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"))
        );
    }

    /// IETF RFC 8288 Appendix B.3: a `type` parameter written as a bare media
    /// type is read like a quoted one, and the context still resolves.
    #[tokio::test]
    async fn a_context_link_with_an_unquoted_type_is_the_request_context() {
        for line in [
            r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type=application/ld+json"#,
            r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
        ] {
            pretty_assertions::assert_eq!(
                extracted(&[line]).await.expect("the context link is admitted"),
                RequestContext::Link(iri("https://example.com/a.jsonld")),
                "{line}"
            );
        }
    }

    /// Clause 5.5.4: a context link whose `type` is empty or not a media type
    /// is malformed, quoted or not.
    #[tokio::test]
    async fn a_context_link_with_an_invalid_type_is_rejected_as_bad_request_data() {
        for line in [
            r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="#,
            r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type=not a type"#,
            r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="not a type""#,
        ] {
            let rejection = extracted(&[line]).await.expect_err("an invalid type is refused");
            assert!(is_bad_request_data(&rejection), "{line}: {rejection:?}");
        }
    }

    /// IETF RFC 8288 section 3.1: a relative target resolves against the
    /// request's own URI.
    #[tokio::test]
    async fn a_relative_context_link_resolves_against_the_request_uri() {
        let sibling = r#"<ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
        pretty_assertions::assert_eq!(
            extracted(&[sibling]).await.expect("a path-relative context link"),
            RequestContext::Link(iri("http://broker.example/ngsi-ld/v1/ctx.jsonld"))
        );
    }

    /// A well-formed `Link` naming a relation other than the JSON-LD context
    /// is not a context, but it is not an error either.
    #[tokio::test]
    async fn a_link_with_another_relation_leaves_the_default_context() {
        pretty_assertions::assert_eq!(
            extracted(&[r#"<https://example.com/other>; rel="stylesheet""#])
                .await
                .expect("another relation is admitted"),
            RequestContext::Default
        );
    }
}
