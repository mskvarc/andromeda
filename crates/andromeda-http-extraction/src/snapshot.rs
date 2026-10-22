//! Snapshot header extractor for NGSI-LD snapshot-scoped requests.
//!
//! Extracts the optional `NGSILD-Snapshot` header from the request, producing
//! a [`SnapshotHeader`] that carries the snapshot URN (or `None` when absent).

use crate::addressed_uri::addressed_uri;
use andromeda_http_headers::ngsild_snapshot::NgsiLdSnapshot;
use andromeda_http_response::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use std::future::{Future, ready};
use thiserror::Error;

/// Axum extractor for the `NGSILD-Snapshot` request header.
///
/// When present, the request is scoped to the frozen data of the specified
/// snapshot. The value must be a valid URN.
#[derive(Clone, Debug, Default)]
pub struct SnapshotHeader(pub Option<Identifier>);

impl SnapshotHeader {
    /// Consume the extractor and return the optional snapshot URN.
    #[must_use]
    pub fn into_inner(self) -> Option<Identifier> {
        self.0
    }
}

/// Decode the `NGSILD-Snapshot` header into the snapshot the request is scoped
/// to, rejecting a value that is not a snapshot URN.
fn resolve_snapshot_header(parts: &Parts) -> Result<SnapshotHeader, ExtractorRejection> {
    match parts.headers.typed_try_get::<NgsiLdSnapshot>() {
        Ok(Some(snapshot)) => Ok(SnapshotHeader(Some(snapshot.into_inner()))),
        Ok(None) => Ok(SnapshotHeader(None)),
        Err(error) => Err(InvalidSnapshotHeader(error).into_rejection(addressed_uri(parts))),
    }
}

/// An `NGSILD-Snapshot` header whose value is not a snapshot URN.
#[derive(Debug, Error)]
#[error("the NGSILD-Snapshot header is not a valid snapshot URN")]
struct InvalidSnapshotHeader(#[source] headers::Error);

impl RequestRefusal for InvalidSnapshotHeader {
    fn error_type(&self) -> NgsiLdErrorType {
        NgsiLdErrorType::BadRequestData
    }
}

impl<S> FromRequestParts<S> for SnapshotHeader
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_snapshot_header(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::FromRequestParts, http::Request};

    #[tokio::test]
    async fn valid_urn_header_extracts_snapshot() {
        let req = Request::builder()
            .uri("/test")
            .header("NGSILD-Snapshot", "urn:ngsi-ld:Snapshot:abc")
            .body(())
            .expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = SnapshotHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        let urn = result.into_inner().expect("should have value");
        pretty_assertions::assert_eq!(urn.as_str(), "urn:ngsi-ld:Snapshot:abc");
    }

    #[tokio::test]
    async fn no_header_returns_none() {
        let req = Request::builder().uri("/test").body(()).expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let result = SnapshotHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        assert!(result.into_inner().is_none());
    }

    #[tokio::test]
    async fn invalid_urn_produces_rejection() {
        let req = Request::builder()
            .uri("/test")
            .header("NGSILD-Snapshot", "not-a-urn")
            .body(())
            .expect("valid request");
        let (mut parts, ()) = req.into_parts();
        let rejection = SnapshotHeader::from_request_parts(&mut parts, &())
            .await
            .expect_err("a value that is no URN is refused");
        let problem = rejection.problem().expect("a classified rejection");
        pretty_assertions::assert_eq!(problem.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(problem.detail(), "the NGSILD-Snapshot header is not a valid snapshot URN");
        pretty_assertions::assert_eq!(problem.instance().map(ToString::to_string).as_deref(), Some("/test"));
    }
}
