//! One partition of the tree: its R-tree and the write records a rebuild
//! respects.

use crate::partitioned_rtree::{Indexed, packed_rtree::PackedRTree};
use rstar::RTreeObject;
use std::collections::HashMap;

/// One partition's R-tree, and the generations of the live mutations applied to
/// it since its last rebuild.
#[derive(Debug)]
pub(super) struct Partition<E: RTreeObject + Indexed> {
    /// The R-tree of this partition's geometries.
    pub(super) tree: PackedRTree<E>,
    /// The generation of the latest live mutation of each object in this
    /// partition (an insertion or a removal) recorded only under
    /// [`WriteTracking::Recorded`](crate::partitioned_rtree::WriteTracking::Recorded).
    pub(super) written: HashMap<<E as Indexed>::Id, u64>,
}

// Manual because deriving `Default` would demand `E: Default`, which no indexed
// entry satisfies; an empty tree and an empty record are what is actually wanted.
impl<E: RTreeObject + Indexed> Default for Partition<E> {
    fn default() -> Partition<E> {
        Partition {
            tree: PackedRTree::default(),
            written: HashMap::new(),
        }
    }
}
