//! Generic listener, connection execution and graceful HTTP draining.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod config;
pub mod error;
pub mod execution;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
