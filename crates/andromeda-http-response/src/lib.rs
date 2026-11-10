//! Centralized framework response construction for Andromeda HTTP bindings.
//!
//! This crate owns every Axum response adapter and HTTP wire projection, including extraction rejections, batch projection and temporal range metadata. A failure is classified once, outside any transport, by the classifiers every reporter of it shares; this crate only binds that classification to the request it answers, as a status, a `Retry-After` and a problem `instance`. A binding supplies its own exhaustive classifier only for the failures it alone answers.

pub mod api_root_path;
pub mod batch;
pub mod batch_projection;
pub mod collection;
pub mod content_range;
pub mod context_hosting_failure;
pub mod declined_representation;
pub mod entity_map_failure;
pub mod extraction_rejection;
mod forwarded_write_failure;
pub mod headers;
pub mod link_target;
mod linked_entity_unresolved;
pub mod linked_resolution_warning;
pub mod method_not_allowed;
pub mod not_acceptable;
pub mod page_ceiling;
pub mod pagination;
pub mod problem_response;
pub mod provision;
pub mod render;
pub mod rendered_response;
pub mod request_refusal;
pub mod single;
pub mod temporal_continuation;
pub mod tenant_state_rejection;
pub mod unreadable_entity_warning;
pub mod version_negotiation;
mod written_body;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity-map",
        "andromeda-failure-classification",
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
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-entity",
        "andromeda-json-literal",
        "andromeda-ngsi-ld-model",
        "andromeda-persistence-failure",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
