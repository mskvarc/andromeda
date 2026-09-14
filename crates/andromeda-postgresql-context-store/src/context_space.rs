//! Resolution of the tenant's live context branch.
//!
//! Under schema-per-snapshot the live branch is the tenant's default schema and
//! needs no lookup, and a Snapshot branch is resolved from its control record by
//! the snapshot store. This module keeps only the live-branch resolver the
//! tenant bootstrap calls; the durable-key resolution and the `context_spaces`
//! deletion the old context-space model needed are gone with that table.

use andromeda_context_space::context_target::ContextTarget;

/// The tenant's live branch, held in the default schema.
///
/// The live branch needs no query: it is the connection's default schema. A
/// Snapshot branch is resolved from its control record by the snapshot store
/// (`SnapshotStore::target`) instead.
#[must_use]
pub const fn resolve_live_target() -> ContextTarget {
    ContextTarget::Live
}
