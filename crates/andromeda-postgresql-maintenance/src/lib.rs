//! Adaptive, allowlisted PostgreSQL inspection and maintenance.
//!
//! The crate owns database-specific measurements, policy, durable controller
//! state, recovery and statement execution. Scheduling remains in broker
//! composition and the task runtime remains database-neutral.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod action;
mod action_measurement;
mod artifact;
/// Compile-time maintenance targets and their fixed statements.
pub mod candidate;
mod capability;
/// The PostgreSQL counters the controller reads and records.
pub mod catalog_counter;
/// One bounded fleet-wide maintenance pass.
pub mod controller;
/// Typed controller failures.
pub mod error;
mod evidence;
mod inspection;
/// Threshold and safety-gate decision policy.
pub mod policy;
mod safety_gate_inspection;
mod scalar_query;
mod state;
/// Bounded on-disk size sampling of the live schema's base tables.
pub mod table_size;
/// Holders of a tenant database's vacuum horizon.
pub mod vacuum_horizon;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["sea-query", "sea-query-sqlx", "sqlx", "tokio"],
}
