//! Temporal query extractor for the `EntityMap` creation resource (clause 6.35.3.1).
//!
//! `GET /temporal/entityMaps` takes the parameters of a temporal query
//! (Table 6.18.3.2-1): the temporal window is mandatory and at least one entity
//! selection criterion must narrow the query (clause 5.14.5.4), which is the
//! validation a temporal query itself undergoes.

use crate::{addressed_uri::addressed_uri, query::classify_query_parse_error};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, parameter_failure::classify_parameter_error};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// Axum extractor for NGSI-LD temporal query parameters with a mandatory window.
#[derive(Debug)]
pub struct TemporalQuery(pub TemporalQueryParameters);

/// Deserialize the temporal query string and validate it as a temporal query.
fn resolve_temporal_query(parts: &Parts) -> Result<TemporalQuery, ExtractorRejection> {
    let uri = addressed_uri(parts);
    let query_str = parts.uri.query().unwrap_or("");
    let params = serde_urlencoded::from_str::<TemporalQueryParameters>(query_str).map_err(|error| {
        let error_text = error.to_string();
        let message = format!("invalid query parameters: {error_text}");
        classify_query_parse_error(&error_text, message, uri.clone())
    })?;

    // Clause 5.14.5.4: a temporal query and one selection criterion are mandatory.
    params.validate(true).map_err(|error| {
        let report = classify_parameter_error(&error);
        ExtractorRejection::Classified(report.with_instance(uri.clone()))
    })?;

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

    async fn extract(uri: &str) -> Result<TemporalQuery, ExtractorRejection> {
        let request = Request::builder().uri(uri).body(Body::empty()).expect("valid request");
        let (mut parts, _) = request.into_parts();
        TemporalQuery::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn a_query_without_a_temporal_window_is_rejected() {
        assert!(extract("/ngsi-ld/v1/temporal/entityMaps?type=Vehicle").await.is_err());
    }

    #[tokio::test]
    async fn a_windowed_query_without_a_selection_criterion_is_rejected() {
        assert!(
            extract("/ngsi-ld/v1/temporal/entityMaps?timerel=after&timeAt=2020-01-01T00:00:00Z")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_windowed_typed_query_is_accepted_with_its_window() {
        let TemporalQuery(params) = extract("/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=5")
            .await
            .expect("valid temporal query");
        assert!(params.timerel.is_some());
        pretty_assertions::assert_eq!(params.last_n, Some(5));
    }
}
