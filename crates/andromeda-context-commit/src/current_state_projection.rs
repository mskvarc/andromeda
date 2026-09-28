//! What one write did to the Entity's current state, which the
//! current-attribute projection follows.

use andromeda_ngsi_ld_model::expanded::removed_instance::RemovedAttributeInstance;
use serde_json::Value;
use std::sync::Arc;

/// The current-state change one write makes, as the projection that answers
/// Core `q` filters has to follow it.
///
/// The projection and the Entity's document always describe the same current
/// state: every write says here which instances now stand, so the two move
/// together in the write's own transaction.
#[derive(Debug)]
pub enum CurrentStateProjection {
    /// The write's history consequences -- the Core observation of the
    /// instances it addressed and the endings of the ones it removed -- are what
    /// current state became. Every Core write is this.
    FromHistory,
    /// The write restated these instances into current state and removed these
    /// ones, without its history recording them as such. A Temporal API write
    /// whose instances stood under clause 4.5.5.3 is this: its history is the
    /// instances as the client sent them, while current state holds only the
    /// most current one of each Attribute instance.
    Restated {
        /// An expanded Entity node carrying the instances that now stand.
        observed: Arc<Value>,
        /// The instances the write removed from current state.
        removed: Vec<RemovedAttributeInstance>,
    },
    /// The write left current state as it was.
    Untouched,
}
