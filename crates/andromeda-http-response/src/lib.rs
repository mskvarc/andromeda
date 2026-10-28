//! Centralized framework response construction for Andromeda HTTP bindings.
//!
//! This crate owns every Axum response adapter and HTTP wire projection, including extraction rejections, batch projection and temporal range metadata. It also owns the classifiers of the policy failures more than one route binding answers (entity, temporal, forwarding, JSON-LD, query and parameter failures), so each such failure has exactly one HTTP classification; a binding supplies its own exhaustive classifier only for the failures it alone answers.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod batch;
pub mod batch_projection;
pub mod classified_report;
pub mod collection;
pub mod content_range;
pub mod context_hosting_failure;
pub mod declined_representation;
pub mod entity_failure;
pub mod entity_map_failure;
pub mod extraction_rejection;
mod forwarded_write_failure;
pub mod forwarding_failure;
pub mod headers;
pub mod internal_failure_summary;
pub mod jsonld_failure;
mod link_target;
mod linked_entity_unresolved;
pub mod linked_resolution_warning;
pub mod method_not_allowed;
pub mod not_acceptable;
pub mod pagination;
pub mod parameter_failure;
pub mod problem_response;
pub mod provision;
pub mod query_failure;
pub mod render;
pub mod rendered_response;
pub mod request_refusal;
pub mod single;
pub mod store_failure;
pub mod temporal_continuation;
pub mod temporal_failure;
pub mod tenant_state_rejection;
pub mod version_negotiation;
mod written_body;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-federation",
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-registration",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-context-commit",
        "andromeda-json-literal",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
