//! Snapshot lifecycle, preparation, frozen-read ports, and expiry policy.

#![deny(missing_docs)]

/// Whether a Snapshot's stored branch can be served by this binary.
pub mod branch_condition;
/// Snapshot operation configuration.
pub mod config;

pub mod error;
pub mod executor;
pub mod expiry;
pub mod prepare;
pub mod query_validation;
/// Snapshot lifecycle service and purge outcomes.
pub mod service;
/// The invariants Snapshot policy relies on.
pub mod snapshot_invariant;
/// Why a Snapshot request is rejected as `BadRequestData`.
pub mod snapshot_validation_reason;
pub mod status;
/// Port for waking durable Snapshot status-notification delivery.
pub mod status_delivery_signal;
pub mod store;
/// An in-memory store double for the policy tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod store_double;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-entity",
        "andromeda-federation",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-expansion",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-retry",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
