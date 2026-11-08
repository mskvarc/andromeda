//! One partition of the tree: its R-tree, the write records a rebuild
//! respects, and the rebuild itself.

use crate::partitioned_rtree::Indexed;
use rstar::{RTree, RTreeObject};
use std::{
    collections::{HashMap, HashSet},
    mem,
};

/// One partition's R-tree, and the generations of the live mutations applied to
/// it since its last rebuild.
#[derive(Debug)]
pub(super) struct Partition<E: RTreeObject + Indexed> {
    /// The R-tree of this partition's geometries.
    pub(super) tree: RTree<E>,
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
            tree: RTree::new(),
            written: HashMap::new(),
        }
    }
}

/// What one partition's rebuild changed for the back-index.
pub(super) struct RebuiltPartition<E: RTreeObject + Indexed> {
    /// Each snapshot entry the partition now holds, as its object and envelope.
    pub(super) installed: Vec<(<E as Indexed>::Id, E::Envelope)>,
    /// The objects the partition no longer holds at all.
    pub(super) departed: HashSet<<E as Indexed>::Id>,
}

impl<E: RTreeObject + Indexed> Partition<E> {
    /// Rebuild this partition from its snapshot, keeping every object written
    /// live after `snapshot_generation`.
    ///
    /// The old tree is drained rather than copied: its entries either move into
    /// the new tree or are dropped, so the partition never holds two copies of
    /// an entry it keeps.
    pub(super) fn rebuild(&mut self, snapshot: Vec<E>, snapshot_generation: u64) -> RebuiltPartition<E> {
        let Partition { tree, written } = self;
        let live: HashSet<&<E as Indexed>::Id> = written
            .iter()
            .filter(|(_, generation)| **generation > snapshot_generation)
            .map(|(id, _)| id)
            .collect();
        let mut entries: Vec<E> = snapshot.into_iter().filter(|entry| !live.contains(entry.id())).collect();
        let mut departed: HashSet<<E as Indexed>::Id> = HashSet::new();
        let mut kept: Vec<E> = Vec::new();
        {
            let installed_ids: HashSet<&<E as Indexed>::Id> = entries.iter().map(Indexed::id).collect();
            for entry in mem::take(tree) {
                if live.contains(entry.id()) {
                    kept.push(entry);
                } else if !installed_ids.contains(entry.id()) {
                    departed.insert(entry.id().clone()); // clone: the entry is dropped at the end of this iteration, so the set owns the identifier
                }
            }
        }
        // clone: the entries move into the new tree, so the installed list owns each identifier
        let installed = entries.iter().map(|entry| (entry.id().clone(), entry.envelope())).collect();
        entries.reserve_exact(kept.len());
        entries.extend(kept);
        *tree = RTree::bulk_load(entries);
        written.retain(|_, generation| *generation > snapshot_generation);
        // A burst of live writes leaves capacity behind that the records no longer use.
        written.shrink_to_fit();
        RebuiltPartition { installed, departed }
    }
}
