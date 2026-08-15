//! Via header extractor for NGSI-LD forwarding loop detection.
//!
//! Extracts the optional `Via` header from the request, producing an
//! [`IncomingVia`] that carries the parsed Via chain (or `None` when absent).
//! Used for loop detection in distributed NGSI-LD forwarding (clause 6.3.18).

use andromeda_http_headers::via::Via;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::convert::Infallible;

/// Axum extractor for the `Via` request header.
///
/// When present, the request carries a chain of intermediary pseudonyms
/// used for forwarding loop detection per ETSI GS CIM 009 clause 6.3.18.
/// Returns `None` when the header is absent or cannot be decoded.
pub struct IncomingVia(pub Option<Via>);

impl IncomingVia {
    /// Consume the extractor and return the optional Via chain.
    pub fn into_inner(self) -> Option<Via> {
        self.0
    }
}

impl<S> FromRequestParts<S> for IncomingVia
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let via = {
            let mut via_values = parts.headers.get_all("via").iter();
            headers::Header::decode(&mut via_values).ok()
        };
        Ok(IncomingVia(via))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use axum::{extract::FromRequestParts, http::Request};

    #[tokio::test]
    async fn no_header_returns_none() {
        let req = Request::builder().uri("/test").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = IncomingVia::from_request_parts(&mut parts, &()).await.expect("infallible");
        assert!(result.into_inner().is_none());
    }

    #[tokio::test]
    async fn valid_via_header_extracts_chain() {
        let req = Request::builder()
            .uri("/test")
            .header("via", "1.1 broker-a, 1.1 broker-b")
            .body(())
            .expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = IncomingVia::from_request_parts(&mut parts, &()).await.expect("infallible");
        let via = result.into_inner().expect("should have value");
        assert!(via.contains_pseudonym(&ContextSourcePseudonym::new("broker-a").expect("valid pseudonym")));
        assert!(via.contains_pseudonym(&ContextSourcePseudonym::new("broker-b").expect("valid pseudonym")));
    }

    #[tokio::test]
    async fn malformed_via_header_returns_none() {
        let req = Request::builder().uri("/test").header("via", "").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
        let result = IncomingVia::from_request_parts(&mut parts, &()).await.expect("infallible");
        assert!(result.into_inner().is_none());
    }
}
