//! The envelope an NGSI-LD request carries around its route and its body.
//!
//! Every NGSI-LD operation reads the same four things off the request before
//! it looks at anything specific to itself: where the client addressed it, the
//! JSON-LD context its `Link` headers named (clause 6.3.5), the `Via` chain it
//! arrived with (clause 6.3.18), and the headers a forwarded copy of it
//! replays to a registered context source (clause 6.3.19). Extracting them one
//! by one puts four parameters on every handler; extracting them together puts
//! one.

use crate::{context::ContextUrl, via::IncomingVia};
use andromeda_http_headers::via::Via;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{
    extract::{FromRequestParts, OriginalUri},
    http::{HeaderMap, Uri, request::Parts},
};

/// The request as it arrived, apart from its route parameters and its body.
///
/// Handlers destructure it in their signature and take whichever fields the
/// operation needs; nothing here is operation-specific, which is why it is one
/// extractor rather than one per member.
pub struct NgsiLdRequestEnvelope {
    /// The URI the client addressed, before any nesting rewrote it. It names
    /// the `instance` of every problem the operation reports and carries the
    /// query string a forwarded copy of the request repeats.
    pub uri: Uri,
    /// The JSON-LD context the request's `Link` headers named (clause 6.3.5),
    /// still to be merged with a body `@context` where the operation has one.
    pub context: ContextUrl,
    /// The `Via` chain the request arrived with, which a forwarded copy
    /// extends so a forwarding loop is detected rather than followed
    /// (clause 6.3.18). `None` when the header is absent or does not decode.
    pub incoming_via: Option<Via>,
    /// The request's headers, replayed onto every forwarded copy so a
    /// registration's `contextSourceInfo` can pass one of them through
    /// (clause 6.3.19).
    pub headers: HeaderMap,
}

impl<S> FromRequestParts<S> for NgsiLdRequestEnvelope
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<NgsiLdRequestEnvelope, Self::Rejection> {
        // OriginalUri is infallible, so unwrap via match.
        let OriginalUri(uri) = match OriginalUri::from_request_parts(parts, state).await {
            Ok(original) => original,
            Err(never) => match never {},
        };
        let context = ContextUrl::from_request_parts(parts, state).await?;
        // IncomingVia is infallible, so unwrap via match.
        let incoming_via = match IncomingVia::from_request_parts(parts, state).await {
            Ok(via) => via.into_inner(),
            Err(never) => match never {},
        };
        // clone: the forwarded copy of the request outlives these parts, which
        // the body extractor that follows consumes.
        let headers = parts.headers.clone();

        Ok(NgsiLdRequestEnvelope {
            uri,
            context,
            incoming_via,
            headers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use axum::http::Request;
    use iri_rs::IriRefBuf;

    /// A request with no `Link` and no `Via` yields an envelope that names
    /// neither, and still carries the addressed URI.
    #[tokio::test]
    async fn a_bare_request_yields_an_envelope_without_context_or_via() {
        let request = Request::builder().uri("/ngsi-ld/v1/entities?type=Vehicle").body(()).expect("a valid request");
        let (mut parts, ()) = request.into_parts();

        let envelope = NgsiLdRequestEnvelope::from_request_parts(&mut parts, &()).await.expect("the envelope extracts");

        pretty_assertions::assert_eq!(envelope.uri.path(), "/ngsi-ld/v1/entities");
        pretty_assertions::assert_eq!(envelope.uri.query(), Some("type=Vehicle"));
        let contexts = envelope.context.into_inner();
        assert!(
            contexts.link_contexts().is_empty(),
            "a bare request carries no Link context: {:?}",
            contexts.link_contexts()
        );
        assert!(envelope.incoming_via.is_none(), "a bare request carries no Via chain");
        assert!(envelope.headers.is_empty(), "a bare request carries no headers");
    }

    /// The envelope reads the context `Link`, the `Via` chain and the header
    /// map off one and the same request.
    #[tokio::test]
    async fn the_envelope_carries_the_context_link_the_via_chain_and_the_headers() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:A100")
            .header("link", r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#)
            .header("via", "1.1 broker-a")
            .header("ngsild-tenant", "acme")
            .body(())
            .expect("a valid request");
        let (mut parts, ()) = request.into_parts();

        let envelope = NgsiLdRequestEnvelope::from_request_parts(&mut parts, &()).await.expect("the envelope extracts");

        let via = envelope.incoming_via.expect("the Via chain decodes");
        assert!(via.contains_pseudonym(&ContextSourcePseudonym::new("broker-a").expect("a valid pseudonym")));
        pretty_assertions::assert_eq!(envelope.headers.get("ngsild-tenant").and_then(|value| value.to_str().ok()), Some("acme"));
        pretty_assertions::assert_eq!(
            envelope.context.into_inner().link_contexts(),
            &[IriRefBuf::new("https://example.com/ctx.jsonld".to_string()).expect("a valid IRI reference")]
        );
    }

    /// Clause 5.5.4: a malformed `Link` header cannot name a context, so the
    /// envelope is rejected rather than extracted without one.
    #[tokio::test]
    async fn a_malformed_link_header_rejects_the_whole_envelope() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("link", "this is not a valid link header")
            .body(())
            .expect("a valid request");
        let (mut parts, ()) = request.into_parts();

        let result = NgsiLdRequestEnvelope::from_request_parts(&mut parts, &()).await;

        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
