//! Required `?q=` extractor for `DELETE /snapshots`
//! (ETSI GS CIM 009 v1.9.1 clauses 5.16.7, 6.36.3.2).
//!
//! The NGSI-LD specification mandates that `DELETE /snapshots` carry a `q`
//! query parameter. Missing or malformed values produce `400 BadRequestData`
//! (clause 6.3.2).

use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_model::snapshot_filter::{SnapshotPurgeFilter, parse_snapshot_q};
use axum::{extract::FromRequestParts, http::request::Parts};
use serde::Deserialize;

/// Axum extractor for the required `?q=` parameter of `DELETE /snapshots`.
#[derive(Debug)]
pub struct SnapshotPurgeQ(pub SnapshotPurgeFilter);

#[derive(Debug, Deserialize)]
/// Raw optional query value before the endpoint-required check.
struct RawPurgeQuery {
    /// Snapshot purge expression supplied as `q`.
    q: Option<String>,
}

impl<S> FromRequestParts<S> for SnapshotPurgeQ
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        let query_str = parts.uri.query().unwrap_or("");
        let raw: RawPurgeQuery = serde_urlencoded::from_str(query_str)
            .map_err(|error| ExtractorRejection::bad_request_data(format!("Invalid query parameters: {error}"), uri.clone()))?;
        let q_value = raw.q.ok_or_else(|| {
            ExtractorRejection::bad_request_data("DELETE /snapshots requires a 'q' query parameter (ETSI GS CIM 009 clause 5.16.7)", uri.clone())
        })?;
        let filter = parse_snapshot_q(&q_value).map_err(|error| ExtractorRejection::bad_request_data(format!("Invalid snapshot purge q: {error}"), uri))?;
        Ok(SnapshotPurgeQ(filter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn missing_q_produces_bad_request_data() {
        let req = Request::builder().uri("/ngsi-ld/v1/snapshots").body(()).expect("request");
        let (mut parts, _) = req.into_parts();
        let result = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }

    #[tokio::test]
    async fn malformed_q_produces_bad_request_data() {
        let req = Request::builder().uri("/ngsi-ld/v1/snapshots?q=bogus").body(()).expect("request");
        let (mut parts, _) = req.into_parts();
        let result = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }

    #[tokio::test]
    async fn valid_q_parses_filter() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/snapshots?q=snapshotPriority%3D%3D5")
            .body(())
            .expect("request");
        let (mut parts, _) = req.into_parts();
        let SnapshotPurgeQ(filter) = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await.expect("should parse");
        pretty_assertions::assert_eq!(filter.terms.len(), 1);
    }
}
