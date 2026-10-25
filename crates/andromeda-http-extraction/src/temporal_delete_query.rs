//! Extractor for the temporal history delete-by-query (the
//! `temporal_delete_by_query` extension).
//!
//! Mirrors [`crate::entity_query`] and [`crate::temporal_query`]: it decodes the
//! query string into [`TemporalDeleteParameters`] and holds it to the delete's
//! own validation (the too-wide rule with the local scope withheld, the
//! `drop`/`keep` and `datasetId`/`deleteAll` exclusivities, and the clause 5.7.3
//! window check). The `TooManyResults` bound is not an extraction concern — it
//! turns on a pre-count the service runs — so it is not decided here.

use crate::{addressed_uri::addressed_uri, query_decode_error::QueryDecodeError};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, parameter_failure::classify_parameter_error, request_refusal::RequestRefusal};
use andromeda_ngsi_ld_request::query::temporal::delete_parameters::TemporalDeleteParameters;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// Axum extractor for the temporal history delete-by-query parameters.
#[derive(Debug)]
pub struct TemporalDeleteQuery(pub TemporalDeleteParameters);

/// Deserialize the delete query string and validate it as a delete-by-query.
fn resolve_temporal_delete_query(parts: &Parts) -> Result<TemporalDeleteQuery, ExtractorRejection> {
    let uri = addressed_uri(parts);
    let query_str = parts.uri.query().unwrap_or("");
    // clone: the decode refusal and the validation refusal each name the request.
    let params = serde_urlencoded::from_str::<TemporalDeleteParameters>(query_str).map_err(|error| QueryDecodeError(error).into_rejection(uri.clone()))?;

    params.validate().map_err(|error| {
        let report = classify_parameter_error(&error);
        ExtractorRejection::Classified(report.with_instance(uri.clone()))
    })?;

    Ok(TemporalDeleteQuery(params))
}

impl<S> FromRequestParts<S> for TemporalDeleteQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_temporal_delete_query(parts))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
    use axum::{body::Body, http::Request};

    async fn extract(uri: &str) -> Result<TemporalDeleteQuery, ExtractorRejection> {
        let request = Request::builder().uri(uri).body(Body::empty()).expect("valid request");
        let (mut parts, _) = request.into_parts();
        TemporalDeleteQuery::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn a_delete_with_a_selector_and_a_window_is_extracted() {
        let TemporalDeleteQuery(params) = extract("/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&drop=speed")
            .await
            .expect("valid delete query");
        assert!(params.entity_type.is_some());
        assert!(params.drop.is_some());
    }

    /// A bare window authorises deleting everyone's history, so it is rejected.
    #[tokio::test]
    async fn a_window_only_delete_is_rejected() {
        assert!(
            extract("/ngsi-ld/v1/temporal/entities?timerel=before&timeAt=2030-01-01T00:00:00Z")
                .await
                .is_err()
        );
    }

    /// A read-only parameter the delete cannot use is refused as an invalid
    /// request (clause 6.3.20 pattern).
    #[tokio::test]
    async fn a_read_only_parameter_is_rejected() {
        let rejection = extract("/ngsi-ld/v1/temporal/entities?type=Vehicle&lastN=5")
            .await
            .expect_err("lastN is refused");
        pretty_assertions::assert_eq!(rejection.problem().map(ProblemDetail::error_type), Some(NgsiLdErrorType::InvalidRequest));
    }
}
