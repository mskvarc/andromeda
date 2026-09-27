//! Verification of the properties a migrated tenant schema itself must hold.
//!
//! These are the properties that are statements about the
//! database rather than about one request: the contract stamped on the tenant,
//! the compact code registries the schema persists numbers from, the absence of
//! procedural logic, the referential state the schema holds, the all-or-nothing
//! shape of every migration phase, the maintenance settings the hot-update
//! relations carry, the TOAST compression the wide columns declare, the
//! explicit name every constraint carries, the octet bound every identifier
//! column holds, the integrity the stored JSON-LD contexts enforce, and the
//! storage policy the temporal hypertables declare and the broker reconciles,
//! and the two concurrency guarantees a live write keeps against unrelated
//! transactions: a commit chosen as a deadlock victim is run again, and a
//! delivery insert waits out a concurrent subscription deletion.
//! Each is executed as a catalogue or constraint query with a pass/fail
//! assertion rather than as a measurement a reader has to interpret.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod code_registry;
mod commit_contention;
mod concurrent_provisioning;
mod constraint_names;
mod delivery_subscription_race;
mod hot_update_maintenance;
mod identifier_bounds;
mod jsonld_context_constraints;
mod migration_failure;
mod migration_ledgers;
mod platform_configuration;
mod procedural_logic;
mod reconciliation;
mod schema_contract;
mod temporal_append_intent;
mod temporal_storage_policy;
mod tenant_registry_constraints;
mod toast_compression;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "tenant_schema";
