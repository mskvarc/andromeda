//! Focused HTTP binding exposing the broker's metrics in Prometheus text format.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod error;
pub mod exposition;
pub mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-telemetry"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
