//! Snapshot lifecycle, preparation, frozen-read ports, and expiry policy.

#![deny(missing_docs)]

/// Snapshot operation configuration.
pub mod config;

pub mod error;
pub mod executor;
pub mod expiry;
pub mod notification;
pub mod prepare;
pub mod query_validation;
/// Snapshot lifecycle service and purge outcomes.
pub mod service;
pub mod status;
pub mod store;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity",
        "andromeda-federation",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["confique", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
