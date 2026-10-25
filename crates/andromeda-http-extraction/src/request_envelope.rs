//! The envelope an NGSI-LD request carries around its route and its body.
//!
//! Every NGSI-LD operation that may be forwarded reads the same three things
//! off the request before it looks at anything specific to itself: where the
//! client addressed it, the `Via` chain it arrived with (clause 6.3.18), and
//! the headers a forwarded copy of it replays to a registered context source
//! (clause 6.3.19). They travel together because a forwarded copy of the
//! request needs all three. The request's context and the `apiRoot` a minted
//! URL is built on are not part of it: an operation extracts
//! [`crate::context::ContextUrl`] or [`crate::api_root::ApiRoot`] only when it
//! reads them.

use crate::{addressed_uri::addressed_uri, via::IncomingVia};
use andromeda_http_headers::via::Via;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, Uri, request::Parts},
};

/// The request as it arrived, apart from its route parameters, its context and
/// its body.
///
/// Handlers destructure it in their signature and take whichever fields the
/// operation needs; nothing here is operation-specific, which is why it is one
/// extractor rather than one per member.
pub struct NgsiLdRequestEnvelope {
    /// The URI the client addressed, before any nesting rewrote it. It names
    /// the `instance` of every problem the operation reports and carries the
    /// query string a forwarded copy of the request repeats.
    pub uri: Uri,
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
        let uri = addressed_uri(parts);
        // IncomingVia is infallible, so unwrap via match.
        let incoming_via = match IncomingVia::from_request_parts(parts, state).await {
            Ok(via) => via.into_inner(),
            Err(never) => match never {},
        };
        // clone: the forwarded copy of the request outlives these parts, which
        // the body extractor that follows consumes.
        let headers = parts.headers.clone();

        Ok(NgsiLdRequestEnvelope { uri, incoming_via, headers })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::request_envelope::NgsiLdRequestEnvelope;
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use axum::{extract::FromRequestParts, http::Request};

    /// A request with no `Via` yields an envelope that names none, and still
    /// carries the addressed URI.
    #[tokio::test]
    async fn a_bare_request_yields_an_envelope_without_via() {
        let request = Request::builder().uri("/ngsi-ld/v1/entities?type=Vehicle").body(()).expect("a valid request");
        let (mut parts, ()) = request.into_parts();

        let envelope = NgsiLdRequestEnvelope::from_request_parts(&mut parts, &()).await.expect("the envelope extracts");

        pretty_assertions::assert_eq!(envelope.uri.path(), "/ngsi-ld/v1/entities");
        pretty_assertions::assert_eq!(envelope.uri.query(), Some("type=Vehicle"));
        assert!(envelope.incoming_via.is_none(), "a bare request carries no Via chain");
        assert!(envelope.headers.is_empty(), "a bare request carries no headers");
    }

    /// The envelope reads the `Via` chain and the header map off one and the
    /// same request.
    #[tokio::test]
    async fn the_envelope_carries_the_via_chain_and_the_headers() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:A100")
            .header("via", "1.1 broker-a")
            .header("ngsild-tenant", "acme")
            .body(())
            .expect("a valid request");
        let (mut parts, ()) = request.into_parts();

        let envelope = NgsiLdRequestEnvelope::from_request_parts(&mut parts, &()).await.expect("the envelope extracts");

        let via = envelope.incoming_via.expect("the Via chain decodes");
        assert!(via.contains_pseudonym(&ContextSourcePseudonym::new("broker-a").expect("a valid pseudonym")));
        pretty_assertions::assert_eq!(envelope.headers.get("ngsild-tenant").and_then(|value| value.to_str().ok()), Some("acme"));
    }

    /// A `Host` that forms no origin does not refuse an operation that mints
    /// no URL: the envelope never reads the `apiRoot`.
    #[tokio::test]
    async fn an_authority_forming_no_origin_does_not_refuse_the_envelope() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header("host", "host:99999")
            .body(())
            .expect("a valid request");
        let (mut parts, ()) = request.into_parts();

        assert!(NgsiLdRequestEnvelope::from_request_parts(&mut parts, &()).await.is_ok());
    }
}
