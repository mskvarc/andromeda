//! The partitioned R-tree both spatial indexes are built from.
//!
//! Entity geometries and subscription reference geometries are stored the same
//! way: one R-tree per [`SpatialKey`] partition behind its own lock, plus a
//! back-index recording where every indexed object currently sits so a removal
//! is a direct tree delete rather than a partition scan. The only thing the two
//! indexes disagree on is what a candidate *means*, which is their own query
//! method; everything structural lives here so the two can never drift apart
//! again.
//!
//! Concurrency invariant: no method ever holds a `buckets` guard and a
//! `placements` guard at the same time. A thread therefore holds at most one of
//! the two maps' shard locks at once, which is what rules out an AB-BA deadlock
//! between them.

use crate::key::SpatialKey;
use dashmap::DashMap;
use parking_lot::RwLock;
use rstar::{RTree, RTreeObject};
use std::{
    collections::HashSet,
    fmt::Debug,
    hash::Hash,
    mem,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// An object the tree can identify without knowing what its payload means.
///
/// Every indexed entry already carries its own identifier, so the partitioned
/// tree keys its ownership records through this trait instead of taking the
/// identifier as a second, redundant parameter that could disagree with the
/// entry.
pub trait Indexed {
    /// The identifier type this object is keyed by.
    type Id: Clone + Eq + Hash + Debug;

    /// This object's identifier.
    fn id(&self) -> &Self::Id;
}

/// One partition's R-tree, tagged with the generation at which it was last
/// modified.
///
/// The generation is what lets a bulk rebuild refuse to overwrite a partition
/// that a live write has touched since the rebuild's snapshot was taken: the
/// index is only ever an optimisation, so it must never lose the newer of two
/// writes to the tree.
#[derive(Debug)]
struct Partition<E: RTreeObject> {
    /// The clock value at this partition's most recent mutation.
    generation: u64,
    /// The R-tree of this partition's geometries.
    tree: RTree<E>,
}

impl<E: RTreeObject> Default for Partition<E> {
    fn default() -> Partition<E> {
        Partition {
            generation: 0,
            tree: RTree::new(),
        }
    }
}

/// Where one object currently sits.
///
/// Storing the whole entry, not just its key, is what makes removal a direct
/// `RTree::remove` rather than a scan of the partition for a matching
/// identifier.
#[derive(Debug)]
struct Placement<E> {
    /// The partition the object was indexed under.
    key: SpatialKey,
    /// The object as the tree holds it, kept so it can be removed by value.
    entry: E,
}

/// A thread-safe R-tree partitioned by [`SpatialKey`], with a back-index for
/// scan-free removal and a per-partition generation for rebuild safety.
#[derive(Debug)]
pub struct PartitionedRTree<E>
where
    E: RTreeObject + Clone + PartialEq + Indexed,
{
    /// One tree per partition.
    buckets: DashMap<SpatialKey, Arc<RwLock<Partition<E>>>>,
    /// Which partitions each object currently appears in, and as which entry.
    placements: DashMap<<E as Indexed>::Id, Vec<Placement<E>>>,
    /// Monotonic clock stamping every mutation, so a bulk rebuild can tell
    /// whether a partition has changed since its snapshot was taken.
    clock: AtomicU64,
}

// Manual because deriving `Default` would demand `E: Default`, which no indexed
// entry satisfies; the map and clock defaults are what is actually wanted.
impl<E> Default for PartitionedRTree<E>
where
    E: RTreeObject + Clone + PartialEq + Indexed,
{
    fn default() -> PartitionedRTree<E> {
        PartitionedRTree {
            buckets: DashMap::new(),
            placements: DashMap::new(),
            clock: AtomicU64::new(0),
        }
    }
}

impl<E> PartitionedRTree<E>
where
    E: RTreeObject + Clone + PartialEq + Indexed,
{
    /// An empty tree.
    #[must_use]
    pub fn new() -> PartitionedRTree<E> {
        PartitionedRTree::default()
    }

    /// The current clock value, to be captured by a rebuild worker *before* it
    /// reads its source snapshot so that any write landing during the read is
    /// stamped with a later generation and cannot be clobbered by the rebuild.
    pub fn current_generation(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }

    /// Advance the clock and return the fresh generation to stamp a mutation.
    fn next_generation(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Replace one object's entries under one partition.
    ///
    /// An object may occupy a partition several times — ETSI GS CIM 009 v1.9.1
    /// clause 4.5.16.1 lets a `GeoProperty` carry one instance per `datasetId`,
    /// and clause 4.10 counts an entity as matching when any one of them meets
    /// the restriction, so every instance has to be reachable. Passing all of
    /// them at once is what makes the replacement atomic with respect to the
    /// back-index: a caller looping over a single-entry insert would have each
    /// call drop the entries the previous one added.
    ///
    /// Only this partition's entries are replaced, not every partition the
    /// object appears in: an object indexed under several keys keeps its other
    /// entries, which is the invariant R2-13 restored.
    pub fn insert_all(&self, key: SpatialKey, id: <E as Indexed>::Id, entries: Vec<E>) {
        self.remove_from_key(&id, &key);
        if entries.is_empty() {
            return;
        }

        let generation = self.next_generation();
        {
            let bucket = self.buckets.entry(key.clone()).or_default(); // clone: bucket key and placement key are separate
            let mut partition = bucket.value().write();
            for entry in &entries {
                partition.tree.insert(entry.clone()); // clone: the tree and the placement each own a copy
            }
            partition.generation = generation;
        }

        let mut entries = entries.into_iter();
        // The final instance takes ownership of the key, so the placements hold
        // one clone fewer than there are instances.
        let last = entries.next_back();
        let mut placements = self.placements.entry(id).or_default();
        for entry in entries {
            placements.value_mut().push(Placement { key: key.clone(), entry }); // clone: one key per placement
        }
        if let Some(entry) = last {
            placements.value_mut().push(Placement { key, entry });
        }
    }

    /// Remove every entry belonging to an object, across every partition.
    pub fn remove(&self, id: &<E as Indexed>::Id) {
        let Some((_, placements)) = self.placements.remove(id) else {
            return;
        };
        for placement in placements {
            self.remove_from_partition(&placement.key, &placement.entry);
        }
    }

    /// Read one partition's tree under its lock, if the partition exists.
    ///
    /// `None` distinguishes a cold partition from an empty answer: the caller
    /// falls through to the store rather than reporting that nothing matched.
    pub fn read_partition<R>(&self, key: &SpatialKey, read: impl FnOnce(&RTree<E>) -> R) -> Option<R> {
        let bucket = self.buckets.get(key)?;
        let partition = bucket.value().read();
        Some(read(&partition.tree))
    }

    /// Replace one partition's contents wholesale, unless a newer write got
    /// there first.
    ///
    /// `snapshot_generation` is the clock value the rebuild worker captured
    /// before reading its source (see [`current_generation`]). If the partition
    /// has been mutated since — its generation exceeds the snapshot's — the
    /// rebuild is stale for it and the swap is refused, so a live insert newer
    /// than the snapshot is never lost. Otherwise the tree is bulk-loaded (a
    /// far better layout than one-at-a-time inserts) under the existing lock, so
    /// a reader sees either the old tree or the new one and never a partial
    /// state, and any object that was in the partition but not in the new
    /// snapshot is pruned from the back-index so its ownership record cannot
    /// leak.
    ///
    /// [`current_generation`]: PartitionedRTree::current_generation
    pub fn bulk_load(&self, key: SpatialKey, entries: Vec<E>, snapshot_generation: u64) {
        // clone: the ownership records outlive the entry list, which the tree consumes
        let incoming: HashSet<<E as Indexed>::Id> = entries.iter().map(|entry| entry.id().clone()).collect();
        // clone: the tree owns its copy; `entries` is kept to refresh the placements
        let tree = RTree::bulk_load(entries.clone());

        let departed: Vec<<E as Indexed>::Id> = {
            let bucket = self.buckets.entry(key.clone()).or_default();
            let mut partition = bucket.value().write();
            if partition.generation > snapshot_generation {
                // A write newer than the snapshot already changed this
                // partition; keeping it is what makes the index only ever an
                // optimisation rather than a source of stale answers.
                return;
            }
            // clone: the departed set outlives the tree borrow it is read from
            let departed = partition
                .tree
                .iter()
                .map(|entry| entry.id().clone())
                .filter(|id| !incoming.contains(id))
                .collect();
            partition.tree = tree;
            partition.generation = snapshot_generation;
            departed
        };

        // The departed objects are by construction absent from `incoming`, so
        // forgetting their records first cannot undo a placement the refresh
        // below installs.
        for id in departed {
            if let Some(mut placements) = self.placements.get_mut(&id) {
                placements.value_mut().retain(|placement| placement.key != key);
            }
        }

        let mut entries = entries.into_iter();
        // The final entry takes ownership of the key, so the placements hold one
        // clone fewer than there are entries.
        let last = entries.next_back();
        for entry in entries {
            self.replace_placement(key.clone(), entry); // clone: one key per placement
        }
        if let Some(entry) = last {
            self.replace_placement(key, entry);
        }
    }

    /// Point one object's placement record at this partition, replacing whatever
    /// entry it previously held there.
    fn replace_placement(&self, key: SpatialKey, entry: E) {
        // clone: the placements map owns the identifier the entry only lends
        let mut placements = self.placements.entry(entry.id().clone()).or_default();
        placements.value_mut().retain(|placement| placement.key != key);
        placements.value_mut().push(Placement { key, entry });
    }

    /// Drop an object from one partition, and forget that it was there.
    fn remove_from_key(&self, id: &<E as Indexed>::Id, key: &SpatialKey) {
        let removed: Vec<E> = {
            let Some(mut placements) = self.placements.get_mut(id) else {
                return;
            };
            let all = mem::take(placements.value_mut());
            let (matching, kept): (Vec<Placement<E>>, Vec<Placement<E>>) = all.into_iter().partition(|placement| &placement.key == key);
            *placements.value_mut() = kept;
            matching.into_iter().map(|placement| placement.entry).collect()
        };
        for entry in &removed {
            self.remove_from_partition(key, entry);
        }
    }

    /// Remove one entry from one partition's tree, advancing that partition's
    /// generation when the entry was actually there.
    ///
    /// The entry comes from the back-index, so this is a direct `RTree::remove`
    /// (O(log n)) rather than a scan of the whole partition for a matching
    /// identifier, and the write lock is held only for the single delete.
    fn remove_from_partition(&self, key: &SpatialKey, entry: &E) {
        let Some(bucket) = self.buckets.get(key) else {
            return;
        };
        let mut partition = bucket.value().write();
        if partition.tree.remove(entry).is_some() {
            partition.generation = self.next_generation();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::entry::SpatialEntry,
        key::SpatialKey,
        partitioned_rtree::{Indexed, PartitionedRTree},
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry::Geometry, position::Position};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};
    use urn_rs::Urn;

    fn urn(raw: &str) -> Urn {
        raw.parse().expect("a well-formed URN")
    }

    fn entry(id: &str, x: f64, y: f64) -> SpatialEntry {
        let geometry = Geometry::new(GeometryForm::Point(Position::new(x, y, None)));
        SpatialEntry::new(urn(id), geometry).expect("a point has a bounding rectangle")
    }

    fn key_a() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/A")),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        )
    }

    fn key_b() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/B")),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        )
    }

    fn count(index: &PartitionedRTree<SpatialEntry>, key: &SpatialKey) -> usize {
        index.read_partition(key, rstar::RTree::size).unwrap_or(0)
    }

    #[test]
    fn inserting_an_object_under_one_key_leaves_another_key_untouched() {
        // The R2-13 invariant, now enforced once: a per-key insert replaces only
        // the partition it targets, so a two-key object survives under both.
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new();
        index.insert_all(
            key_a(),
            entry("urn:ngsi-ld:X:1", 0.0, 0.0).id().clone(),
            vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)],
        );
        index.insert_all(
            key_b(),
            entry("urn:ngsi-ld:X:1", 1.0, 1.0).id().clone(),
            vec![entry("urn:ngsi-ld:X:1", 1.0, 1.0)],
        );
        pretty_assertions::assert_eq!(count(&index, &key_a()), 1);
        pretty_assertions::assert_eq!(count(&index, &key_b()), 1);
    }

    #[test]
    fn removing_one_object_from_a_partition_leaves_the_rest() {
        // Direct removal touches only the removed entry, not the whole partition.
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new();
        index.insert_all(
            key_a(),
            entry("urn:ngsi-ld:X:1", 0.0, 0.0).id().clone(),
            vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)],
        );
        index.insert_all(
            key_a(),
            entry("urn:ngsi-ld:X:2", 5.0, 5.0).id().clone(),
            vec![entry("urn:ngsi-ld:X:2", 5.0, 5.0)],
        );
        index.remove(&urn("urn:ngsi-ld:X:1"));
        pretty_assertions::assert_eq!(count(&index, &key_a()), 1);
    }
}
