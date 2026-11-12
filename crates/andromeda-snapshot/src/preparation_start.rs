//! Port for starting a created or cloned Snapshot's preparation at once.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Starts the preparation of a Snapshot a request just created or cloned, so
/// the Snapshot does not wait for the scheduled preparation sweep.
///
/// Starting returns at once and the preparation runs elsewhere. A start for a
/// Snapshot whose preparation is already under way may be dropped, since that
/// preparation covers it; the scheduled sweep stays the guarantee that every
/// `preparing` Snapshot is executed.
pub trait PreparationStart: Send + Sync {
    /// Start the preparation of the Snapshot `snapshot` now.
    fn start(&self, snapshot: &Identifier);
}
