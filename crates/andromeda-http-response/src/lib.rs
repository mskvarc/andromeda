//! Centralized framework response construction for Andromeda HTTP bindings.
//!
//! This crate owns every Axum response adapter and HTTP wire projection, including extraction rejections, batch projection and temporal range metadata. It deliberately chooses no policy-error category: each route binding supplies an exhaustive classifier or an already classified report.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod batch;
pub mod batch_projection;
pub mod classified_report;
pub mod collection;
pub mod content_range;
pub mod declined_representation;
pub mod extraction_rejection;
pub mod headers;
pub mod internal_failure_summary;
pub mod linked_resolution_warning;
pub mod not_acceptable;
pub mod pagination;
pub mod problem_response;
pub mod provision;
pub mod render;
pub mod single;
pub mod tenant_state_rejection;
pub mod version_negotiation;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-federation",
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
