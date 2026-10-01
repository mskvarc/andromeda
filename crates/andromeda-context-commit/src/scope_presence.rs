//! Whether an entity held a Scope (clause 4.18).

/// Whether an entity held a Scope when a write removed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopePresence {
    /// The entity held one or more Scopes.
    Held,
    /// The entity held no Scope.
    Absent,
}
