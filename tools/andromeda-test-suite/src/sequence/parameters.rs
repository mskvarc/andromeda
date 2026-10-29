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
    ParameterError,
    PurgeEntitiesParameters,
    QueryEntitiesParameters,
    QueryEntitiesPostParameters,
    QueryTemporalEvolutionParameters,
    QueryTemporalEvolutionPostParameters,
    RetrieveEntityParameters,
    RetrieveTemporalEvolutionParameters,
};

/// Why the parameters are refused.
#[derive(Debug, thiserror::Error)]
#[error("the {operation} parameter model refuses the query: {source}")]
pub struct ParameterRefusal {
    /// The operation.
    operation: ParameterOperation,
    /// The model's reason.
    #[source]
    source: ParameterError,
}

/// An operation whose query parameters the crate reads with a typed model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum ParameterOperation {
    /// Query Entities.
    #[strum(serialize = "Query Entities")]
    QueryEntities,
    /// Retrieve Entity.
    #[strum(serialize = "Retrieve Entity")]
    RetrieveEntity,
    /// Query Temporal Evolution.
    #[strum(serialize = "Query Temporal Evolution")]
    QueryTemporalEvolution,
    /// Retrieve Temporal Evolution.
    #[strum(serialize = "Retrieve Temporal Evolution")]
    RetrieveTemporalEvolution,
    /// Query Entities by POST.
    #[strum(serialize = "Query Entities by POST")]
    QueryEntitiesPost,
    /// Query Temporal Evolution by POST.
    #[strum(serialize = "Query Temporal Evolution by POST")]
    QueryTemporalEvolutionPost,
    /// Append Attributes.
    #[strum(serialize = "Append Attributes")]
    AppendAttributes,
    /// Delete Attribute.
    #[strum(serialize = "Delete Attribute")]
    DeleteAttribute,
    /// Batch Upsert.
    #[strum(serialize = "Batch Upsert")]
    BatchUpsert,
    /// Batch Update.
    #[strum(serialize = "Batch Update")]
    BatchUpdate,
    /// Purge Entities.
    #[strum(serialize = "Purge Entities")]
    PurgeEntities,
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
    let refusal = |operation: ParameterOperation| move |source: ParameterError| ParameterRefusal { operation, source };
    let method = method.as_str();
    match (method, segments.as_slice()) {
        ("GET", ["entities"]) => QueryEntitiesParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::QueryEntities)),
        ("GET", ["entities", _]) => RetrieveEntityParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::RetrieveEntity)),
        ("GET", ["temporal", "entities"]) => QueryTemporalEvolutionParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::QueryTemporalEvolution)),
        ("GET", ["temporal", "entities", _]) => RetrieveTemporalEvolutionParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::RetrieveTemporalEvolution)),
        ("POST", ["entityOperations", "query"]) => QueryEntitiesPostParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::QueryEntitiesPost)),
        ("POST", ["temporal", "entityOperations", "query"]) => QueryTemporalEvolutionPostParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::QueryTemporalEvolutionPost)),
        ("POST", ["entities", _, "attrs"]) => AppendAttributesParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::AppendAttributes)),
        ("DELETE", ["entities", _, "attrs", _]) => DeleteAttributeParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::DeleteAttribute)),
        ("POST", ["entityOperations", "upsert"]) => BatchUpsertParameters::parse(query).map(drop).map_err(refusal(ParameterOperation::BatchUpsert)),
        ("POST", ["entityOperations", "update"]) => BatchUpdateParameters::parse(query).map(drop).map_err(refusal(ParameterOperation::BatchUpdate)),
        ("DELETE", ["entities"]) => PurgeEntitiesParameters::parse(query)
            .map(drop)
            .map_err(refusal(ParameterOperation::PurgeEntities)),
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
