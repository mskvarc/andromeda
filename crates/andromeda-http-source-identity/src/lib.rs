//! Focused HTTP binding for the NGSI-LD context source identity resource.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Context Source identity configuration.
pub mod config;
/// The default tenant's context resolution and hosting.
pub mod default_tenant_contexts;
pub mod error;
pub mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-extensions",
        "andromeda-http-context-registry",
        "andromeda-http-extraction",
        "andromeda-http-response",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-request",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: ["andromeda-context-registry", "andromeda-ngsi-ld-query", "andromeda-test-harness"],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
