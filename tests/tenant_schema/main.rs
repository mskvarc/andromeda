//! Verification of the properties a migrated tenant schema itself must hold.
//!
//! These are the verification matrix rows that are statements about the
//! database rather than about one request: the contract stamped on the tenant,
//! the compact code registries the schema persists numbers from, the absence of
//! procedural logic, the referential state the schema holds, the all-or-nothing
//! shape of every migration phase, the maintenance settings the hot-update
//! relations carry, the TOAST compression the wide columns declare, and the
//! storage policy the temporal hypertables declare and the broker reconciles. Each is executed as a catalogue or constraint query with a
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
mod temporal_storage_policy;
mod toast_compression;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "tenant_schema";

/// Resolve the durable live context-space key installed by the tenant schema.
pub async fn live_context_space_key(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT context_space_key FROM context_spaces WHERE kind = 'live'")
        .fetch_one(pool)
        .await
        .expect("tenant live context space must exist")
}
