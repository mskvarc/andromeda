//! Verification of the properties a migrated tenant schema itself must hold.
//!
//! These are the verification matrix rows that are statements about the
//! database rather than about one request: the contract stamped on the tenant,
//! the compact code registries the schema persists numbers from, the absence of
//! procedural logic, the referential state the schema holds, the all-or-nothing
//! shape of every migration phase, and the maintenance settings the hot-update
//! relations carry. Each is executed as a catalogue or constraint query with a
//! pass/fail assertion rather than as a measurement a reader has to interpret.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod code_registry;
mod hot_update_maintenance;
mod migration_failure;
mod procedural_logic;
mod reconciliation;
mod schema_contract;
mod temporal_append_intent;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "tenant_schema";
