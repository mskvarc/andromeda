//! Entity query extractor with NGSI-LD filter validation.
//!
//! Parses and validates query parameters for entity listing endpoints,
//! enforcing the filter requirement from ETSI GS CIM 009 clause 5.7.2.4:
//! at least one filter or `local=true` must be present.

use crate::{
    core::error::{AndromedaError, ErrorKind},
    http::{extractor::error::ExtractorRejection, parameter::query::entity::EntityQueryParameters},
};
use axum::{extract::FromRequestParts, http::request::Parts};

/// Axum extractor for NGSI-LD entity query parameters.
///
/// Deserializes the query string into [`EntityQueryParameters`], validates
/// option values and parameter constraints, and enforces the NGSI-LD 5.7.2.4
/// requirement that at least one filter (type, id, idPattern, q, etc.) or
/// `local=true` must be present.
#[derive(Debug)]
pub struct EntityQuery(pub EntityQueryParameters);

impl<S> FromRequestParts<S> for EntityQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        // Parse query string manually
        let query_str = parts.uri.query().unwrap_or("");
        let params = serde_urlencoded::from_str::<EntityQueryParameters>(query_str)
            .map_err(|error| ExtractorRejection::invalid_request(format!("Invalid query string: {}", error), uri.clone()))?;

        // Validate options and other parameter values. QueryError classifies
        // each variant as either MalformedRequest or QueryValidation when
        // converted to AndromedaError; we forward that distinction to the
        // spec-defined InvalidRequest / BadRequestData NGSI-LD error types.
        params.validate().map_err(|e| {
            let andromeda: AndromedaError = e.into();
            match andromeda.kind() {
                ErrorKind::MalformedRequest(_) => ExtractorRejection::invalid_request(andromeda.to_string(), uri.clone()),
                _ => ExtractorRejection::bad_request_data(andromeda.to_string(), uri.clone()),
            }
        })?;

        // Per NGSI-LD spec 5.7.2.4: at least one filter OR local=true must be provided
        if !params.local.unwrap_or(false) && !params.has_any_filter() {
            return Err(ExtractorRejection::invalid_request(
                "At least one of type, id, idPattern, q, or local=true must be provided",
                uri,
            ));
        }

        Ok(EntityQuery(params))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn rejects_missing_filters() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn allows_empty_query_with_local_true() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities?local=true")
            .body(axum::body::Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok(), "expected Ok when local=true, even without filters");
    }
}
