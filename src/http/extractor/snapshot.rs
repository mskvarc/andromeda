//! Snapshot header extractor for NGSI-LD snapshot-scoped requests.
//!
//! Extracts the optional `NGSILD-Snapshot` header from the request, producing
//! a [`SnapshotHeader`] that carries the snapshot URN (or `None` when absent).

use crate::http::{extractor::error::ExtractorRejection, header::ngsild_snapshot::NgsiLdSnapshot};
use axum::{extract::FromRequestParts, http::request::Parts};
use headers::HeaderMapExt;
use urn::Urn;

/// Axum extractor for the `NGSILD-Snapshot` request header.
///
/// When present, the request is scoped to the frozen data of the specified
/// snapshot. The value must be a valid URN.
#[derive(Clone, Debug, Default)]
pub struct SnapshotHeader(pub Option<Urn>);

impl SnapshotHeader {
    /// Consume the extractor and return the optional snapshot URN.
    pub fn into_inner(self) -> Option<Urn> {
        self.0
    }
}

impl<S> FromRequestParts<S> for SnapshotHeader
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();

        match parts.headers.typed_try_get::<NgsiLdSnapshot>() {
            Ok(Some(snapshot)) => Ok(Self(Some(snapshot.into_inner()))),
            Ok(None) => Ok(Self(None)),
            Err(_) => Err(ExtractorRejection::bad_request_data("NGSILD-Snapshot header must be a valid snapshot URN", uri)),
        }
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
        let (mut parts, _) = req.into_parts();
        let result = SnapshotHeader::from_request_parts(&mut parts, &()).await.expect("should succeed");
        let urn = result.into_inner().expect("should have value");
        assert_eq!(urn.as_str(), "urn:ngsi-ld:Snapshot:abc");
    }

    #[tokio::test]
    async fn no_header_returns_none() {
        let req = Request::builder().uri("/test").body(()).expect("valid request");
        let (mut parts, _) = req.into_parts();
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
        let (mut parts, _) = req.into_parts();
        let result = SnapshotHeader::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
