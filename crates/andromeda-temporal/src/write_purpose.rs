//! What one temporal write is for.

/// What one temporal write is for, and therefore what an absent Temporal
/// Evolution means for it.
///
/// Clause 5.6.16 lets a client delete the Temporal Evolution of an Entity on
/// its own — "If the target Temporal Evolution of an Entity exists locally, the
/// entire Temporal Evolution of the Entity shall be removed" — while clause
/// 5.6.6 removes current state on its own, "used to remove the entity locally
/// if it exists". An Entity whose current state stands while its Temporal
/// Evolution is gone is therefore a state the API itself lets a client reach,
/// in either order, and not a broker malfunction. What a write does when it
/// meets that state depends on what the write is for, which is the one thing
/// the persisted catalogue cannot infer from the rows it is handed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporalWritePurpose {
    /// The write records observations, so it extends the Temporal Evolution.
    ///
    /// Clause 4.5.1 makes at least one Entity type part of what an Entity is,
    /// so an observation that declares none can only extend an evolution that
    /// already exists: nothing in the write says what to create one with, and
    /// inventing a type would state something the client never wrote.
    Observation,
    /// The write records only the end of instances the Entity no longer holds.
    ///
    /// Clauses 4.5.7 and 4.5.8 record a deletion as one more instance of a
    /// sequence — "In case the Property is deleted, an instance of the Property
    /// is recorded with its value set to the URI `urn:ngsi-ld:null` and the
    /// deletedAt Temporal Property set" — so a deletion that meets no sequence
    /// has no instance to end. It records nothing and reports success, because
    /// the state it is recording the end of is already the state it wanted.
    Deletion,
}
