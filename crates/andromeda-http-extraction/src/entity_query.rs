//! Entity query extractor with NGSI-LD filter validation.
//!
//! Parses and validates query parameters for entity listing endpoints,
//! enforcing the filter requirement from ETSI GS CIM 009 clause 5.7.2.4:
//! at least one filter or `local=true` must be present.

use crate::query::classify_query_parse_error;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// Axum extractor for NGSI-LD entity query parameters.
///
/// Deserializes the query string into [`EntityQueryParameters`], validates
/// option values and parameter constraints, and enforces the NGSI-LD 5.7.2.4
/// requirement that at least one filter (type, id, idPattern, q, etc.) or
/// `local=true` must be present.
#[derive(Debug)]
pub struct EntityQuery(pub EntityQueryParameters);

/// Deserialize the entity query string and enforce the clause 5.7.2.4 filter
/// requirement.
fn resolve_entity_query(parts: &Parts) -> Result<EntityQuery, ExtractorRejection> {
    let uri = parts.uri.clone();
    let query_str = parts.uri.query().unwrap_or("");
    let params = serde_urlencoded::from_str::<EntityQueryParameters>(query_str).map_err(|error| {
        let error_text = error.to_string();
        let message = format!("Invalid query parameters: {error_text}");
        classify_query_parse_error(&error_text, message, uri.clone())
    })?;

    // Clause 5.7.2.4: at least one filter OR local=true must be provided.
    if !params.local.unwrap_or(false) && !params.has_any_filter() {
        return Err(ExtractorRejection::bad_request_data(
            "At least one of type, id, idPattern, q, or local=true must be provided",
            uri,
        ));
    }

    Ok(EntityQuery(params))
}

impl<S> FromRequestParts<S> for EntityQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(resolve_entity_query(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};

    #[tokio::test]
    async fn rejects_missing_filters() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }

    #[tokio::test]
    async fn allows_empty_query_with_local_true() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities?local=true").body(Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok(), "expected Ok when local=true, even without filters");
    }

    #[tokio::test]
    async fn rejects_unknown_parameters_as_invalid_request() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities?type=Building&invalidParam=invalidValue")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn rejects_invalid_parameter_values_as_bad_request_data() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities?type=Building&limit=invalid")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
    }
}
