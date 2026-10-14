//! Temporal query extractor for the `EntityMap` creation resource (clause 6.35.3.1).
//!
//! `GET /temporal/entityMaps` takes the parameters of a temporal query
//! (Table 6.18.3.2-1). They are read here and validated by the binding, which
//! knows whether the broker retrieves Linked Entities on the temporal
//! operations (clause 5.14.5.4).

use crate::{addressed_uri::addressed_uri, query::classify_query_parse_error};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// Axum extractor for NGSI-LD temporal query parameters.
#[derive(Debug)]
pub struct TemporalQuery(pub TemporalQueryParameters);

/// Deserialize the temporal query string.
fn resolve_temporal_query(parts: &Parts) -> Result<TemporalQuery, ExtractorRejection> {
    let query_str = parts.uri.query().unwrap_or("");
    let params = serde_urlencoded::from_str::<TemporalQueryParameters>(query_str).map_err(|error| classify_query_parse_error(error, addressed_uri(parts)))?;
    Ok(TemporalQuery(params))
}

impl<S> FromRequestParts<S> for TemporalQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_temporal_query(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use std::num::NonZeroU32;

    async fn extract(uri: &str) -> Result<TemporalQuery, ExtractorRejection> {
        let request = Request::builder().uri(uri).body(Body::empty()).expect("valid request");
        let (mut parts, _) = request.into_parts();
        TemporalQuery::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn a_query_string_that_does_not_decode_is_rejected() {
        assert!(extract("/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&lastN=abc").await.is_err());
    }

    #[tokio::test]
    async fn a_windowed_typed_query_is_accepted_with_its_window() {
        let TemporalQuery(params) = extract("/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=5")
            .await
            .expect("valid temporal query");
        assert!(params.timerel.is_some());
        pretty_assertions::assert_eq!(params.last_n.map(NonZeroU32::get), Some(5));
    }

    /// The `lastN` parameter is a "Positive integer" (Table 6.18.3.2-1), so zero
    /// is a value the operation cannot take.
    #[tokio::test]
    async fn a_zero_last_n_is_bad_request_data() {
        let rejection = extract("/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=0").await;

        assert!(matches!(rejection, Err(ExtractorRejection::BadRequestData { .. })), "{rejection:?}");
    }
}
