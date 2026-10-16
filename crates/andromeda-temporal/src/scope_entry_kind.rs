//! Whether one scope entry of a temporal append intent is an event or an
//! assertion of the Scope in force.

/// What one scope entry states about the Entity's Scope (clause 4.18).
///
/// Clause 4.18 makes a Scope valid "from the time it has been set until the
/// time it has been explicitly removed by an update or delete operation". A
/// write that sets or removes a Scope is an event of the Temporal Evolution
/// whatever the history holds. A write that only states the Entity is in a
/// Scope sets nothing when that Scope is already in force at the write, and
/// only the Temporal Evolution knows whether it is: current state may name a
/// Scope the history no longer holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeEntryKind {
    /// An event recorded as it stands: a Core write that changed the Scope or
    /// created the Entity, a reified scope instance of a Temporal API write
    /// (dated by its own `observedAt`, clauses 4.5.6 and 4.5.7), or a removal.
    Event,
    /// A statement that the Entity is in the Scope: a Core overwrite, merge or
    /// replacement restating the Scope the Entity held, or the non-reified
    /// `scope` of a Temporal API write. It records an event only where the
    /// Scope in force at its instant differs.
    Assertion,
}
