//! Snapshot lifecycle, preparation, frozen-read ports, and expiry policy.

/// Snapshot operation configuration.
pub mod error;
pub mod executor;
pub mod expiry;
pub mod preparation_start;
pub mod prepare;
pub mod query_validation;
/// Admitting a Snapshot's `receiverInfo` against its endpoint.
pub mod receiver_info_admission;
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
mod store_double;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-entity",
        "andromeda-failure-classification",
        "andromeda-federation",
        "andromeda-http-headers",
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
