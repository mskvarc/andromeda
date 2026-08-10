//! Focused HTTP serialization and routing for active broker component reports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod error;
pub mod handler;
pub mod report;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-component-report",
        "andromeda-http-response",
        "andromeda-ngsi-ld-lexicon",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
