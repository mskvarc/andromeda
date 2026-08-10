//! Entity Map request validation owned by its HTTP binding.

use crate::error::bad_request_data;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::http::Uri;

/// Require one Entity Map membership filter as specified by clause 5.14.4.4.
pub(crate) fn validate_entity_map_has_filter(query: &EntityQueryParameters, uri: &Uri) -> Result<(), ProblemResponse> {
    if query.has_any_filter() {
        Ok(())
    } else {
        Err(bad_request_data(
            "At least one query filter must be provided (type, attrs, q, georel, scopeQ, or id)",
            uri,
        ))
    }
}
