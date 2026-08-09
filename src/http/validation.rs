//! Shared parsing and validation helpers for HTTP handlers.
//!
//! Functions in this module extract and validate common path/query parameters
//! used across multiple handler modules (entities, temporal, subscriptions,
//! context source registrations, snapshots, entity maps).

use crate::error::AndromedaError;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::http::Uri;

/// Reject an entity-map creation that would match everything.
///
/// Clause 5.14.4.4 requires at least one of `type`, `attrs`, `q`, `georel`,
/// `scopeQ` or `id`: an entity map is a stable page over a result set, and a
/// result set with no filter is every entity the broker holds.
///
/// # Errors
///
/// Returns an `EntityMapValidation` problem when the query names no filter.
pub fn validate_entity_map_has_filter(query: &EntityQueryParameters, uri: &Uri) -> Result<(), ProblemResponse> {
    if query.has_any_filter() {
        return Ok(());
    }

    Err(ProblemResponse::from(
        AndromedaError::entity_map_validation("At least one query filter must be provided (type, attrs, q, georel, scopeQ, or id)").with_instance(uri),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri() -> Uri {
        "/test".parse().expect("valid test URI")
    }

    #[test]
    fn entity_map_creation_rejects_a_query_without_a_filter() {
        let query = serde_urlencoded::from_str::<EntityQueryParameters>("").expect("empty query parameters");
        assert!(validate_entity_map_has_filter(&query, &test_uri()).is_err());
    }

    #[test]
    fn entity_map_creation_accepts_a_query_with_an_identifier_filter() {
        let query = serde_urlencoded::from_str::<EntityQueryParameters>("id=urn:ngsi-ld:Vehicle:1").expect("valid query parameters");
        assert!(validate_entity_map_has_filter(&query, &test_uri()).is_ok());
    }
}
