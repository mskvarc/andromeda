//! Focused HTTP binding for broker health aggregation.

pub mod error;
pub mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-failure-classification",
        "andromeda-health",
        "andromeda-http-response",
        "andromeda-ngsi-ld-lexicon",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
