//! Focused HTTP binding for the NGSI-LD context source identity resource.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Context Source identity configuration.
pub mod config;
pub mod error;
pub mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-extraction",
        "andromeda-http-response",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-render",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum", "confique"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
