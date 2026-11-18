//! What the back-index records of one entry.

use crate::key::SpatialKey;
use rstar::RTreeObject;
use std::sync::Arc;

/// Where one entry of an object sits: enough to find it in its tree again.
#[derive(Debug)]
pub(super) struct Placement<E: RTreeObject> {
    /// The partition the entry was indexed under.
    pub(super) key: Arc<SpatialKey>,
    /// The entry's envelope, which the removal descends by.
    pub(super) envelope: E::Envelope,
}
