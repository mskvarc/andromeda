//! Adaptive, allowlisted PostgreSQL inspection and maintenance.
//!
//! The crate owns database-specific measurements, policy, durable controller
//! state, recovery and statement execution. Scheduling remains in broker
//! composition and the task runtime remains database-neutral.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Log one observation finding at its level, its decision name as the
/// `decision` field, followed by the given fields and message. The caller
/// imports `ObservationLevel`.
macro_rules! observation_event {
    ($finding:expr, $($event:tt)+) => {{
        let finding = $finding;
        let decision = finding.decision_name();
        match finding.level {
            ObservationLevel::Warning => tracing::warn!(decision, $($event)+),
            ObservationLevel::Error => tracing::error!(decision, $($event)+),
        }
    }};
}

mod action;
mod action_measurement;
mod action_policy;
mod artifact;
mod autovacuum_health;
mod autovacuum_trigger;
mod branch;
mod branch_selection;
mod btree_estimate;
/// Compile-time maintenance targets and their fixed statements.
pub mod candidate;
mod capability;
/// The PostgreSQL counters the controller reads and records.
pub mod catalog_counter;
mod columnstore_edition;
mod columnstore_partiality;
/// One bounded fleet-wide maintenance pass.
pub mod controller;
/// Typed controller failures.
pub mod error;
mod evidence;
/// The facts of the broker's configuration the controller checks the cluster
/// against.
pub mod expected_deployment;
mod extension_observation;
mod fleet_observation;
mod fleet_triage;
mod freeze_policy;
mod freeze_report;
mod gin_density;
/// On-disk size of the history chunk each temporal hypertable converts next.
pub mod history_chunk_size;
mod inspection;
mod observation;
mod observed_heap;
/// Threshold and safety-gate decision policy.
pub mod policy;
mod prepared_action;
mod replication_slot;
mod safety_gate_inspection;
mod state;
mod statement_statistics;
mod statistics_freshness;
/// Bounded on-disk size sampling of the live schema's base tables.
pub mod table_size;
mod timescale_observation;
mod unvisited_database;
/// Holders of a tenant database's vacuum horizon.
pub mod vacuum_horizon;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-context-space",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-branch",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-retry",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["sea-query", "sqlx", "tokio"],
}
