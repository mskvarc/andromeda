//! HTTP binding for snapshot lifecycle and purge operations.

pub mod error;
mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity",
        "andromeda-failure-classification",
        "andromeda-federation",
        "andromeda-http-extraction",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-request",
        "andromeda-snapshot",
        "andromeda-temporal",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: ["andromeda-persistence-failure"],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
