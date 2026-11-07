//! Required `?q=` extractor for `DELETE /snapshots`
//! (ETSI GS CIM 009 v1.9.1 clauses 5.16.7, 6.36.3.2).
//!
//! The NGSI-LD specification mandates that `DELETE /snapshots` carry a `q`
//! query parameter. Missing or malformed values produce `400 BadRequestData`
//! (clause 6.3.2), and any other parameter `400 InvalidRequest` (clause
//! 6.3.20).

use crate::{addressed_uri::addressed_uri, query_decoding::decode_query, query_refusal::QueryRefusal};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
use andromeda_ngsi_ld_model::snapshot_filter::{SnapshotPurgeFilter, SnapshotQParseError};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::{
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
};
use axum::{extract::FromRequestParts, http::request::Parts};
use serde::Deserialize;
use std::future::{Future, ready};
use thiserror::Error;

/// Why the query of `DELETE /snapshots` names no purge filter.
#[derive(Debug, Error)]
enum SnapshotPurgeRefusal {
    /// The query string does not decode into parameters.
    #[error(transparent)]
    Query(#[from] QueryRefusal),
    /// Clause 5.16.7: the operation requires a `q` parameter and the request
    /// carries none.
    #[error("a delete of the snapshots requires a 'q' query parameter")]
    MissingQ,
    /// The `q` expression does not parse as a snapshot filter.
    #[error("invalid snapshot purge q: {0}")]
    InvalidQ(#[source] SnapshotQParseError),
}

impl RequestRefusal for SnapshotPurgeRefusal {
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            SnapshotPurgeRefusal::Query(error) => error.error_type(),
            SnapshotPurgeRefusal::MissingQ | SnapshotPurgeRefusal::InvalidQ(_) => NgsiLdErrorType::BadRequestData,
        }
    }
}

/// Axum extractor for the required `?q=` parameter of `DELETE /snapshots`.
#[derive(Debug)]
pub struct SnapshotPurgeQ(pub SnapshotPurgeFilter);

/// Raw optional query value before the endpoint-required check.
///
/// Table 6.36.3.2-1 defines `q` alone, so any other parameter is the clause
/// 6.3.20 `InvalidRequest`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPurgeQuery {
    /// Snapshot purge expression supplied as `q`.
    q: Option<String>,
}

query_parameter_set!(RawPurgeQuery { Q });

/// Read the mandatory `q` expression off the request, rejecting a missing or
/// malformed value as `BadRequestData` (clauses 5.16.7 and 6.3.2).
fn resolve_purge_query(parts: &Parts) -> Result<SnapshotPurgeQ, ExtractorRejection> {
    parse_purge_query(parts).map_err(|refusal| refusal.into_rejection(addressed_uri(parts)))
}

/// The purge filter the request's query string names.
fn parse_purge_query(parts: &Parts) -> Result<SnapshotPurgeQ, SnapshotPurgeRefusal> {
    let raw: RawPurgeQuery = decode_query(parts.uri.query().unwrap_or(""))?;
    let q_value = raw.q.ok_or(SnapshotPurgeRefusal::MissingQ)?;
    let filter = q_value.parse::<SnapshotPurgeFilter>().map_err(SnapshotPurgeRefusal::InvalidQ)?;
    Ok(SnapshotPurgeQ(filter))
}

impl<S> FromRequestParts<S> for SnapshotPurgeQ
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_purge_query(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn missing_q_produces_bad_request_data() {
        let req = Request::builder().uri("/ngsi-ld/v1/snapshots").body(()).expect("request");
        let (mut parts, ()) = req.into_parts();
        let rejection = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await.expect_err("a missing q is refused");
        let problem = rejection.problem().expect("a classified rejection");
        pretty_assertions::assert_eq!(problem.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(problem.detail(), "a delete of the snapshots requires a 'q' query parameter");
        pretty_assertions::assert_eq!(problem.instance().map(ToString::to_string).as_deref(), Some("/ngsi-ld/v1/snapshots"));
    }

    #[tokio::test]
    async fn malformed_q_produces_bad_request_data() {
        let req = Request::builder().uri("/ngsi-ld/v1/snapshots?q=bogus").body(()).expect("request");
        let (mut parts, ()) = req.into_parts();
        let rejection = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await.expect_err("a malformed q is refused");
        let problem = rejection.problem().expect("a classified rejection");
        pretty_assertions::assert_eq!(problem.error_type(), NgsiLdErrorType::BadRequestData);
        assert!(problem.detail().starts_with("invalid snapshot purge q: "), "{}", problem.detail());
    }

    /// Table 6.36.3.2-1 defines `q` alone, so a parameter beside it is one
    /// the operation does not define (clause 6.3.20).
    #[tokio::test]
    async fn a_parameter_beside_q_produces_invalid_request() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/snapshots?q=snapshotPriority%3D%3D5&limit=3")
            .body(())
            .expect("request");
        let (mut parts, ()) = req.into_parts();
        let rejection = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await.expect_err("limit is refused");
        let problem = rejection.problem().expect("a classified rejection");
        pretty_assertions::assert_eq!(problem.error_type(), NgsiLdErrorType::InvalidRequest);
        assert!(problem.detail().contains("limit"), "{}", problem.detail());
    }

    #[tokio::test]
    async fn valid_q_parses_filter() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/snapshots?q=snapshotPriority%3D%3D5")
            .body(())
            .expect("request");
        let (mut parts, ()) = req.into_parts();
        let SnapshotPurgeQ(filter) = SnapshotPurgeQ::from_request_parts(&mut parts, &()).await.expect("should parse");
        pretty_assertions::assert_eq!(filter, "snapshotPriority==5".parse::<SnapshotPurgeFilter>().expect("a purge filter"));
    }
}
