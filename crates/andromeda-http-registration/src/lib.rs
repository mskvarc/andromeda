//! HTTP binding for NGSI-LD context-source registration lifecycle operations.

pub mod error;
mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-failure-classification",
        "andromeda-http-context-registry",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-request",
        "andromeda-registration",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: ["andromeda-ngsi-ld-geometry", "andromeda-persistence-failure"],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
