//! Whether an Entity holds a Scope (clause 4.18).

/// Whether an Entity holds a Scope: one or more paths under its `scope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopePresence {
    /// The Entity holds one or more Scopes.
    Held,
    /// The Entity holds no Scope.
    Absent,
}
