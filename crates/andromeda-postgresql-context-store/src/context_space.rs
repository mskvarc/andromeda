//! Resolution of the tenant's live context branch.
//!
//! Under schema-per-snapshot the live branch is the tenant's default schema and needs no lookup; a Snapshot branch is resolved from its control record by the snapshot store.

use andromeda_context_space::context_target::ContextTarget;

/// The live branch needs no query: it is the connection's default schema.
#[must_use]
pub const fn resolve_live_target() -> ContextTarget {
    ContextTarget::Live
}
