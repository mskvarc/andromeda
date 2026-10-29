//! What the back-index records of one entry, and how the tree finds the entry
//! again from it.

use crate::{key::SpatialKey, partitioned_rtree::Indexed};
use rstar::{Envelope, RTreeObject, SelectionFunction};
use std::sync::Arc;

/// Where one entry of an object sits: enough to find it in its tree again.
#[derive(Debug)]
pub(super) struct Placement<E: RTreeObject> {
    /// The partition the entry was indexed under.
    pub(super) key: Arc<SpatialKey>,
    /// The entry's envelope, which the removal descends by.
    pub(super) envelope: E::Envelope,
}

/// Selects an entry of one object by the envelope it was indexed under.
///
/// Two entries of an object that share a partition and an envelope are
/// interchangeable here: an object's entries in a partition always leave
/// together, so whichever is found first is one of the leaving ones.
pub(super) struct EntryOf<'a, E: RTreeObject + Indexed> {
    /// The object the entry belongs to.
    pub(super) id: &'a <E as Indexed>::Id,
    /// The envelope the entry was indexed under.
    pub(super) envelope: &'a E::Envelope,
}

impl<E: RTreeObject + Indexed> SelectionFunction<E> for EntryOf<'_, E> {
    fn should_unpack_parent(&self, envelope: &E::Envelope) -> bool {
        envelope.contains_envelope(self.envelope)
    }

    fn should_unpack_leaf(&self, leaf: &E) -> bool {
        leaf.id() == self.id && leaf.envelope() == *self.envelope
    }
}
