//! The `Link` headers of a request, read once.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.5 reads a request's context "from a Link
//! header [7] as mandated by JSON-LD [2]": one link, with the JSON-LD context
//! relation. The JSON-LD 1.1 API reads a document with several context links
//! as an error ("multiple context link headers", section 9.4.1 step 5), and
//! IETF RFC 8288 section 3.1 resolves a relative link target against the
//! request's own URI. Clause 6.3.21 announces the NGSI-LD version of a payload
//! by a `profile` link in the same header.
//!
//! [`RequestLinks`] is that one reading, done once per request and kept as a
//! request extension: the [`validate_request_links`] layer every NGSI-LD
//! request passes through rejects a malformed `Link` for every verb, and every
//! extractor that needs the context or the profile ([`crate::context`],
//! [`crate::jsonld`], [`crate::entity_batch`], [`crate::profile`]) reads the
//! kept result rather than parsing the header again.

use crate::{
    addressed_uri::{addressed_request_uri, addressed_uri},
    api_root::{ApiRootError, request_origin},
    classification::classify_context_link_error,
};
use andromeda_http_headers::link::{error::LinkParseError, header::Link};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use axum::{
    extract::Request,
    http::{
        header::{LINK, ToStrError},
        request::Parts,
        uri::PathAndQuery,
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use iri_rs::{InvalidIri, IriBuf, IriParseError, IriRefBuf};
use std::str::FromStr;
use thiserror::Error;

/// How the request carries its payload, which decides whether a context
/// `Link` may accompany it (clause 6.3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadCarrier {
    /// No payload, or an `application/json` one: the context comes from the
    /// `Link` header.
    LinkHeader,
    /// An `application/ld+json` payload: the context comes from the body, and a
    /// context `Link` is refused.
    LdJsonBody,
}

/// Why a request's `Link` headers name no context the request can be read
/// against.
#[derive(Debug, Error)]
pub enum ContextLinkError {
    /// A `Link` header is not visible ASCII.
    #[error("the Link header is not visible ASCII")]
    NotVisibleAscii(#[source] ToStrError),
    /// A `Link` header is not a well-formed link (IETF RFC 8288).
    #[error("the Link header is malformed")]
    Malformed(#[source] LinkParseError),
    /// The request names more than one JSON-LD context link.
    #[error("the request names {count} JSON-LD context links, and a request may name at most one")]
    SeveralContexts {
        /// How many context links the request named.
        count: usize,
    },
    /// The context link names a relative reference, and the request names no
    /// authority its own URI could be built from.
    #[error("the JSON-LD context link names a relative reference, and the request names no authority to resolve it against")]
    NoRequestAuthority,
    /// The authority the request names forms no origin its URI could be
    /// built from.
    #[error("the JSON-LD context link names a relative reference, and the request authority forms no origin")]
    InvalidOrigin(#[source] ApiRootError),
    /// The origin and the path the request addressed do not form an IRI.
    #[error("the JSON-LD context link names a relative reference, and the request URI is not an IRI")]
    RequestUriNotIri(#[source] InvalidIri<String>),
    /// The relative reference and the request URI do not recompose into an
    /// IRI reference (IETF RFC 3986 section 5.2).
    #[error("the JSON-LD context link names a relative reference that does not resolve against the request URI")]
    Unresolvable(#[source] IriParseError),
    /// The resolved reference is not an absolute IRI.
    #[error("the JSON-LD context link does not resolve to an absolute IRI")]
    ResolvedNotAbsolute(#[source] InvalidIri<String>),
    /// Clause 6.3.5: an `application/ld+json` body names its own context, so a
    /// context link beside it is refused.
    #[error("application/ld+json must not be combined with a Link context header")]
    CombinedWithLdJson,
}

/// What a request's `Link` headers say: the one JSON-LD context they name, if
/// any, and the NGSI-LD version a `profile` link announces (clause 6.3.21), if
/// any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestLinks {
    /// The one JSON-LD context link, as the absolute IRI its target resolves
    /// to.
    context: Option<IriBuf>,
    /// The NGSI-LD version the first profile-bearing link announces, as
    /// written.
    profile: Option<SpecVersion>,
}

impl RequestLinks {
    /// The request's `Link` headers, read once and kept on the request.
    ///
    /// Every `Link` field line is parsed; a line that is not visible ASCII or
    /// not a well-formed link refuses the request (clause 5.5.4), and so do
    /// several context link-values across all lines together, identical or
    /// not: JSON-LD counts links, not distinct targets. A link with another
    /// relation (the clause 6.3.21 `profile`, a pagination link) is not a
    /// context and is not counted. A relative target is resolved against the
    /// request's own URI (IETF RFC 8288 section 3.1).
    ///
    /// # Errors
    ///
    /// Returns the [`ContextLinkError`] the headers draw.
    pub fn of(parts: &mut Parts) -> Result<RequestLinks, ContextLinkError> {
        if let Some(read) = parts.extensions.get::<RequestLinks>() {
            return Ok(read.clone()); // clone: the reading stays in the request extensions for the extractors that follow
        }
        let read = read_request_links(parts)?;
        parts.extensions.insert(read.clone());
        Ok(read)
    }

    /// The one context the request's `Link` headers name, if any.
    #[must_use]
    pub const fn context(&self) -> Option<&IriBuf> {
        self.context.as_ref()
    }

    /// The context link a request carrying its payload by `carrier` is read
    /// against.
    ///
    /// # Errors
    ///
    /// Returns [`ContextLinkError::CombinedWithLdJson`] for a context link
    /// beside an `application/ld+json` body (clause 6.3.5).
    pub fn context_for(self, carrier: PayloadCarrier) -> Result<Option<IriBuf>, ContextLinkError> {
        match (carrier, self.context) {
            (PayloadCarrier::LdJsonBody, Some(_)) => Err(ContextLinkError::CombinedWithLdJson),
            (PayloadCarrier::LdJsonBody, None) => Ok(None),
            (PayloadCarrier::LinkHeader, context) => Ok(context),
        }
    }

    /// The NGSI-LD version a `profile` link announces, as written.
    #[must_use]
    pub const fn profile(&self) -> Option<SpecVersion> {
        self.profile
    }
}

/// Parse every `Link` field line of the request into the one reading
/// [`RequestLinks::of`] keeps.
fn read_request_links(parts: &Parts) -> Result<RequestLinks, ContextLinkError> {
    let mut targets: Vec<IriRefBuf> = Vec::new();
    let mut profile = None;
    for value in parts.headers.get_all(LINK) {
        let link = Link::from_str(value.to_str().map_err(ContextLinkError::NotVisibleAscii)?).map_err(ContextLinkError::Malformed)?;
        profile = profile.or_else(|| link.ngsi_ld_profile_version());
        targets.extend(link.context_urls().cloned()); // clone: the parsed header is dropped at the end of the iteration
    }
    let count = targets.len();
    let mut targets = targets.into_iter();
    let context = match (targets.next(), targets.next()) {
        (None, _) => None,
        (Some(single), None) => Some(absolute_target(&single, parts)?),
        (Some(_), Some(_)) => return Err(ContextLinkError::SeveralContexts { count }),
    };
    Ok(RequestLinks { context, profile })
}

/// The absolute IRI a context link's target names: the target itself when it
/// is absolute, else the target resolved against the request's own URI.
fn absolute_target(target: &IriRefBuf, parts: &Parts) -> Result<IriBuf, ContextLinkError> {
    if let Some(absolute) = target.as_iri() {
        return Ok(IriBuf::from(absolute));
    }
    let base = effective_request_uri(parts)?;
    let resolved = target.resolved(&base).map_err(ContextLinkError::Unresolvable)?;
    IriBuf::try_from(resolved).map_err(ContextLinkError::ResolvedNotAbsolute)
}

/// The URI the request targets, absolute: the origin the request reveals and
/// the path and query the client addressed (IETF RFC 9110 section 7.1).
fn effective_request_uri(parts: &Parts) -> Result<IriBuf, ContextLinkError> {
    let origin = request_origin(parts)
        .map_err(ContextLinkError::InvalidOrigin)?
        .ok_or(ContextLinkError::NoRequestAuthority)?;
    let addressed = addressed_uri(parts);
    let path_and_query = addressed.path_and_query().map_or("/", PathAndQuery::as_str);
    IriBuf::new(format!("{}{path_and_query}", origin.as_str().trim_end_matches('/'))).map_err(ContextLinkError::RequestUriNotIri)
}

/// Refuse a request whose `Link` headers are unreadable, before any route
/// looks at it.
///
/// Clause 5.5.4 validates every request's context, whatever its verb, and a
/// route that never names the request's context still answers a malformed
/// `Link`, or several context links, as `BadRequestData` rather than ignoring
/// them. The reading is kept on the request, so no extractor parses the header
/// again.
pub async fn validate_request_links(request: Request, next: Next) -> Response {
    let instance = addressed_request_uri(&request);
    let (mut parts, body) = request.into_parts();
    match RequestLinks::of(&mut parts) {
        Ok(_) => next.run(Request::from_parts(parts, body)).await,
        Err(error) => ExtractorRejection::Classified(ProblemResponse::classified(classify_context_link_error(&error), instance)).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::request_links::{ContextLinkError, PayloadCarrier, RequestLinks, validate_request_links};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use axum::{
        Router,
        body::Body,
        http::{HeaderValue, Request, StatusCode, request::Parts},
        middleware::from_fn,
        routing::get,
    };
    use iri_rs::IriBuf;
    use tower::ServiceExt;

    const CONTEXT_A: &str = r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
    const CONTEXT_B: &str = r#"<https://example.com/b.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
    const PROFILE: &str = r#"<https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="profile""#;

    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    /// The parts of a request to `/ngsi-ld/v1/entities?type=T` on
    /// `broker.example`, carrying one `Link` line per entry of `link_lines`.
    fn parts_with_links(link_lines: &[&str]) -> Parts {
        let mut request = Request::builder().uri("/ngsi-ld/v1/entities?type=T").header("host", "broker.example");
        for line in link_lines {
            request = request.header("Link", *line);
        }
        request.body(()).expect("a valid request").into_parts().0
    }

    #[test]
    fn no_link_header_names_no_context_and_no_profile() {
        pretty_assertions::assert_eq!(
            RequestLinks::of(&mut parts_with_links(&[])).expect("no Link is admitted"),
            RequestLinks::default()
        );
    }

    #[test]
    fn one_context_link_and_a_profile_link_are_read_together() {
        let links = RequestLinks::of(&mut parts_with_links(&[CONTEXT_A, PROFILE])).expect("both are admitted");
        pretty_assertions::assert_eq!(links.context(), Some(&iri("https://example.com/a.jsonld")));
        pretty_assertions::assert_eq!(links.profile(), Some(SpecVersion::V1_8));
    }

    /// The reading is kept on the request: a second read answers the kept one
    /// even after the header itself is gone.
    #[test]
    fn the_reading_is_kept_on_the_request() {
        let mut parts = parts_with_links(&[CONTEXT_A]);
        let first = RequestLinks::of(&mut parts).expect("one context link");
        parts.headers.remove("link");
        pretty_assertions::assert_eq!(RequestLinks::of(&mut parts).expect("the kept reading"), first);
    }

    /// Clause 6.3.5 and JSON-LD 1.1 API section 9.4.1: two context links, in
    /// two lines or in one, are refused, identical or not.
    #[test]
    fn several_context_links_are_refused_with_their_count() {
        for lines in [
            vec![CONTEXT_A, CONTEXT_B],
            vec![CONTEXT_A, CONTEXT_A],
            vec![&format!("{CONTEXT_A}, {CONTEXT_B}") as &str],
        ] {
            assert!(
                matches!(
                    RequestLinks::of(&mut parts_with_links(&lines)),
                    Err(ContextLinkError::SeveralContexts { count: 2 })
                ),
                "{lines:?}"
            );
        }
    }

    #[test]
    fn a_malformed_or_non_ascii_link_is_refused() {
        assert!(matches!(
            RequestLinks::of(&mut parts_with_links(&["not a link"])),
            Err(ContextLinkError::Malformed(_))
        ));
        let mut parts = parts_with_links(&[]);
        parts.headers.insert(
            "link",
            HeaderValue::from_bytes("<https://example.com/\u{e9}.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"".as_bytes())
                .expect("an opaque header value"),
        );
        assert!(matches!(RequestLinks::of(&mut parts), Err(ContextLinkError::NotVisibleAscii(_))));
    }

    /// IETF RFC 8288 section 3.1: a relative target resolves against the
    /// request's own URI.
    #[test]
    fn a_relative_context_link_resolves_against_the_request_uri() {
        let rooted = r#"</ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:1>; rel="http://www.w3.org/ns/json-ld#context""#;
        pretty_assertions::assert_eq!(
            RequestLinks::of(&mut parts_with_links(&[rooted]))
                .expect("a root-relative context link")
                .context(),
            Some(&iri("http://broker.example/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:1"))
        );
    }

    #[test]
    fn a_relative_context_link_without_a_request_authority_is_refused_as_such() {
        let mut parts = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("Link", r#"<ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#)
            .body(())
            .expect("a valid request")
            .into_parts()
            .0;
        assert!(matches!(RequestLinks::of(&mut parts), Err(ContextLinkError::NoRequestAuthority)));
    }

    /// An authority that forms no origin is its own refusal, carrying the
    /// origin failure as its source.
    #[test]
    fn a_relative_context_link_on_an_authority_forming_no_origin_is_refused_as_such() {
        let mut parts = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("host", "host:99999")
            .header("Link", r#"<ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#)
            .body(())
            .expect("a valid request")
            .into_parts()
            .0;
        assert!(matches!(RequestLinks::of(&mut parts), Err(ContextLinkError::InvalidOrigin(_))));
    }

    /// Clause 6.3.5: a context link beside an `application/ld+json` body is
    /// refused, no link at all is not.
    #[test]
    fn an_ld_json_body_refuses_a_context_link() {
        let links = RequestLinks::of(&mut parts_with_links(&[CONTEXT_A])).expect("one context link");
        assert!(matches!(
            links.clone().context_for(PayloadCarrier::LdJsonBody),
            Err(ContextLinkError::CombinedWithLdJson)
        ));
        pretty_assertions::assert_eq!(
            links.context_for(PayloadCarrier::LinkHeader).expect("the Link carrier"),
            Some(iri("https://example.com/a.jsonld"))
        );
        pretty_assertions::assert_eq!(
            RequestLinks::of(&mut parts_with_links(&[PROFILE]))
                .expect("a profile link")
                .context_for(PayloadCarrier::LdJsonBody)
                .expect("a profile link beside a body"),
            None
        );
    }

    /// The layer answers every route: a malformed or doubled context `Link`
    /// never reaches a handler.
    #[tokio::test]
    async fn the_layer_refuses_unreadable_links_before_any_route() {
        let router = Router::new()
            .route("/ngsi-ld/v1/entities", get(|| async { StatusCode::OK }))
            .layer(from_fn(validate_request_links));
        for (lines, expected) in [
            (vec![], StatusCode::OK),
            (vec![CONTEXT_A], StatusCode::OK),
            (vec![CONTEXT_A, CONTEXT_B], StatusCode::BAD_REQUEST),
            (vec!["not a link"], StatusCode::BAD_REQUEST),
        ] {
            let mut request = Request::builder().uri("/ngsi-ld/v1/entities").header("host", "broker.example");
            for line in &lines {
                request = request.header("Link", *line);
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).expect("a valid request"))
                .await
                .expect("the router answers");
            pretty_assertions::assert_eq!(response.status(), expected, "{lines:?}");
        }
    }
}
