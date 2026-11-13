//! Every schema of a tenant database brought through the migrations in
//! version lockstep: the live branch and every Snapshot branch of a v1.0.0
//! tenant, resumed where an earlier upgrade stopped, stopped where a Snapshot
//! branch cannot follow and resumed once it can, with a branch schema no
//! control record names left alone, replayed by a Snapshot created
//! afterwards, a database no migration has reached yet, one whose ledger
//! records a branch migration other than the embedded one, and the platform
//! database, which runs the platform set first.

mod edited_migration;
mod failing_snapshot;
mod fresh_tenant;
mod orphan_branch;
mod platform_database;
mod resumed_upgrade;
mod sealed_branch;
mod snapshot_replay;
mod snapshot_seals;
