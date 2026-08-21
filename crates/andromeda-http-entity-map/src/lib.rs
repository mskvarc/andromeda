//! HTTP binding for current and temporal Entity Map lifecycle operations.

#![deny(missing_docs)]

pub mod error;
mod handler;
pub mod routes;
pub mod state;
mod validation;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-federation",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-request",
        "andromeda-temporal",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
