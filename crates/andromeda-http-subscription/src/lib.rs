//! HTTP binding for entity and context-source subscription lifecycle and streaming routes.

#![deny(missing_docs)]

/// Streaming subscription transport configuration.
pub mod config;
mod csource_subscription;
pub mod error;
pub mod routes;
mod sse;
pub mod state;
mod subscription;
mod ws;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-delivery",
        "andromeda-http-extraction",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-request",
        "andromeda-subscription",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum", "confique", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
