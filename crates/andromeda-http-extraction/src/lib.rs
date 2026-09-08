//! Centralized Axum request extraction for Andromeda's HTTP bindings.
//!
//! Every `FromRequest` and `FromRequestParts` implementation belongs here, including endpoint-specific body and query extractors and generic focused tenant-state extraction. Extractors may decode policy input types but never invoke policy services or select operations.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod accept;
pub mod classification;
pub mod context;
pub mod context_target;
pub mod entity_batch;
pub mod entity_query;
pub mod json_body;
pub mod jsonld;
pub mod language;
pub mod origin;
pub mod prefer;
pub mod profile;
pub mod protocol;
pub mod query;
pub mod request_envelope;
pub mod scoped_response;
pub mod snapshot;
pub mod snapshot_purge;
pub mod snapshot_scope_guard;
pub mod temporal_query;
pub mod tenant;
pub mod tenant_state;
pub mod typed_path;
pub mod via;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
