//! The entity-query extractor: the query string, decoded and classified.
//!
//! What the decoded parameters then have to satisfy -- the clause 5.7.2.4
//! narrowing rule among it -- belongs to
//! [`EntityQueryParameters::validate`](andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters),
//! which every binding of the operation calls. This seam owns only the decoding
//! and the clause 6.3.20 classification of a failure to decode.

use crate::query::classify_query_parse_error;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::future::{Future, ready};

/// Axum extractor for NGSI-LD entity query parameters.
///
/// Deserializes the query string into [`EntityQueryParameters`] and classifies
/// a failure to decode it (clause 6.3.20).
#[derive(Debug)]
pub struct EntityQuery(pub EntityQueryParameters);

/// Deserialize the entity query string, classifying a failure to decode it.
fn resolve_entity_query(parts: &Parts) -> Result<EntityQuery, ExtractorRejection> {
    let uri = parts.uri.clone();
    let query_str = parts.uri.query().unwrap_or("");
    let params = serde_urlencoded::from_str::<EntityQueryParameters>(query_str).map_err(|error| {
        let error_text = error.to_string();
        let message = format!("Invalid query parameters: {error_text}");
        classify_query_parse_error(&error_text, message, uri.clone())
    })?;

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

    /// A query string the parameter set can decode is extracted whatever the
    /// clause 5.7.2.4 narrowing rule will make of it: that rule is applied by
    /// the parameter set's own validation, which the operation runs.
    #[tokio::test]
    async fn a_decodable_query_string_is_extracted() {
        for query in ["", "local=true", "type=Vehicle", "id=urn:ngsi-ld:Vehicle:1"] {
            let req = Request::builder().uri(format!("/ngsi-ld/v1/entities?{query}")).body(Body::empty()).unwrap();
            let (mut parts, _) = req.into_parts();

            let result = EntityQuery::from_request_parts(&mut parts, &()).await;
            assert!(result.is_ok(), "'{query}' decodes");
        }
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
