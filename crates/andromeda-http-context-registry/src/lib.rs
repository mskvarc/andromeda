//! HTTP binding for hosted and cached JSON-LD Context lifecycle operations.

#![deny(missing_docs)]

pub mod error;
mod handler;
pub mod hosted_context_reference;
pub mod hosted_context_url;
pub mod response_context_host;
pub mod response_rendering;
pub mod routes;
mod serve_tenant;
mod served_document;
pub mod state;
pub mod tenant_context_hosting;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-registry",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-persistence-failure",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-model",
        "andromeda-test-harness",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
