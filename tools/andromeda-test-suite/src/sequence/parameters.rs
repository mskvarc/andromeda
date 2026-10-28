//! The query parameters of a request read by the `ngsi-ld` crate's parameter model of its operation, so a test that
//! sends well-formed parameters sends only what the typed model accepts.
//!
//! The operation is told by method and path. Operations the crate has no parameter model for (the listings of
//! registrations, subscriptions, types, attributes, contexts and entity maps) send their parameters as written.

use http::Method;
use ngsi_ld::http::operation::{
    AppendAttributesParameters,
    BatchUpdateParameters,
    BatchUpsertParameters,
    DeleteAttributeParameters,
    PurgeEntitiesParameters,
    QueryEntitiesParameters,
    QueryEntitiesPostParameters,
    QueryTemporalEvolutionParameters,
    QueryTemporalEvolutionPostParameters,
    RetrieveEntityParameters,
    RetrieveTemporalEvolutionParameters,
};

/// Why the parameters are refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the {operation} parameter model refuses {query:?}: {reason}")]
pub struct ParameterRefusal {
    /// The operation.
    operation: &'static str,
    /// The query string.
    query: String,
    /// The model's reason.
    reason: String,
}

/// The segments of a path below the API root, or none when the path is not below it.
fn below_root(path: &str) -> Option<Vec<&str>> {
    let rest = path.split_once("/ngsi-ld/v1/")?.1;
    Some(rest.split('/').filter(|segment| !segment.is_empty()).collect())
}

/// Reads `query`, the encoded query string of a `method` request to `path`, with the parameter model of its operation.
///
/// # Errors
///
/// Returns [`ParameterRefusal`] when the model refuses the parameters.
pub fn check(method: &Method, path: &str, query: &str) -> Result<(), ParameterRefusal> {
    let Some(segments) = below_root(path) else { return Ok(()) };
    let refusal = |operation: &'static str| {
        move |reason: String| ParameterRefusal {
            operation,
            query: query.to_owned(),
            reason,
        }
    };
    let method = method.as_str();
    match (method, segments.as_slice()) {
        ("GET", ["entities"]) => QueryEntitiesParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Query Entities")(e.to_string())),
        ("GET", ["entities", _]) => RetrieveEntityParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Retrieve Entity")(e.to_string())),
        ("GET", ["temporal", "entities"]) => QueryTemporalEvolutionParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Query Temporal Evolution")(e.to_string())),
        ("GET", ["temporal", "entities", _]) => RetrieveTemporalEvolutionParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Retrieve Temporal Evolution")(e.to_string())),
        ("POST", ["entityOperations", "query"]) => QueryEntitiesPostParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Query Entities by POST")(e.to_string())),
        ("POST", ["temporal", "entityOperations", "query"]) => QueryTemporalEvolutionPostParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Query Temporal Evolution by POST")(e.to_string())),
        ("POST", ["entities", _, "attrs"]) => AppendAttributesParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Append Attributes")(e.to_string())),
        ("DELETE", ["entities", _, "attrs", _]) => DeleteAttributeParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Delete Attribute")(e.to_string())),
        ("POST", ["entityOperations", "upsert"]) => BatchUpsertParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Batch Upsert")(e.to_string())),
        ("POST", ["entityOperations", "update"]) => BatchUpdateParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Batch Update")(e.to_string())),
        ("DELETE", ["entities"]) => PurgeEntitiesParameters::parse(query)
            .map(drop)
            .map_err(|e| refusal("Purge Entities")(e.to_string())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::parameters::check;
    use googletest::prelude::*;
    use http::Method;

    #[gtest]
    fn a_well_formed_query_is_accepted() {
        expect_that!(check(&Method::GET, "/ngsi-ld/v1/entities", "type=Building&limit=5"), ok(anything()));
    }

    #[gtest]
    fn a_listing_without_model_is_not_read() {
        expect_that!(check(&Method::GET, "/ngsi-ld/v1/subscriptions", "limit=zero"), ok(anything()));
    }

    #[gtest]
    fn a_malformed_limit_is_refused() {
        expect_that!(check(&Method::GET, "/ngsi-ld/v1/entities", "type=Building&limit=zero"), err(anything()));
    }
}
