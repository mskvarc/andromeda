//! NGSI-LD entity, entity-operation, type-discovery, and attribute-discovery HTTP binding.
//!
//! This crate owns the handlers, entity-specific extraction, focused tenant state, route inventory, batch projection, and exhaustive classification of the focused errors consumed by clauses 5.6.1 through 5.6.10.

pub mod attribute;
mod batch_payload_admission;
mod compact_screening;
mod discovered_response;
/// Handlers of the `/entities` resources: single-entity and attribute writes, retrieval, Query Entities and Purge Entities.
mod entity;
/// Handlers of the `/entityOperations` resources: the batch writes and the POST binding of Query Entities.
mod entity_operations;
pub mod entity_type;
pub mod error;
mod forwarded_write_query;
mod join;
mod read_rendering;
mod request_projection;
pub mod routes;
pub mod state;
mod written_payload_admission;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-extensions",
        "andromeda-failure-classification",
        "andromeda-federation",
        "andromeda-http-context-registry",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-linked-retrieval",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-expansion",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-posted-query",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-persistence-failure",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
