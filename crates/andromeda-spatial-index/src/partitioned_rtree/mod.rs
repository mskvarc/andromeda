//! The partitioned R-tree both spatial indexes are built from.
//!
//! Entity geometries and subscription reference geometries are stored the same
//! way: one R-tree per [`SpatialKey`] partition behind its own lock, plus a
//! back-index recording where every indexed object currently sits so a removal
//! is a direct tree descent rather than a partition scan. The only thing the two
//! indexes disagree on is what a candidate *means*, which is their own query
//! method; everything structural lives here so the two cannot drift apart.
//!
//! The tree holds the only copy of an entry. The back-index records, per entry,
//! the partition and the envelope the tree finds it under, and shares the
//! partition's key, so an object costs the back-index an identifier and a few
//! words however large its geometry is.
//!
//! Every mutation of one object is applied partition by partition, and within a
//! partition the object's outgoing entries leave and its incoming entries arrive
//! under one write lock. A reader of a partition therefore sees the object
//! either as it was or as it now is, never absent in between.
//!
//! Concurrency invariants:
//!
//! - No method ever holds a `buckets` guard and a `placements` guard at the same
//!   time, and no partition lock is taken while a `placements` guard is held. A
//!   thread therefore holds at most one of the two maps' shard locks at once,
//!   which is what rules out an AB-BA deadlock between them.
//! - The mutations of one object are serialised by the caller. The back-index
//!   of an object is read and rewritten around the tree changes, so two
//!   concurrent mutations of the same object could each rewrite it from a
//!   stale reading; every caller in the broker already applies one object's
//!   changes in commit order, one at a time.

mod partition;
mod placement;

use crate::{
    key::SpatialKey,
    partitioned_rtree::{
        partition::Partition,
        placement::{EntryOf, Placement},
    },
};
use dashmap::{DashMap, mapref::entry::Entry};
use parking_lot::RwLock;
use rstar::{RTree, RTreeObject};
use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
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

/// Whether live mutations are remembered for a later rebuild to respect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteTracking {
    /// Every live mutation records, per partition it touches, the generation it
    /// was applied at. A [`PartitionedRTree::rebuild`] from a snapshot read
    /// before that generation keeps the live state of the object instead of
    /// the snapshot's, and the rebuild forgets every record its snapshot
    /// supersedes, so the records never outlive one rebuild interval.
    Recorded,
    /// Nothing is recorded. A tree built this way is never rebuilt from a
    /// snapshot, so there is nothing a record would protect.
    Unrecorded,
}

/// One partition, and the key every placement in it shares.
#[derive(Debug)]
struct Bucket<E: RTreeObject + Indexed> {
    /// The partition's key, one allocation however many placements name it.
    key: Arc<SpatialKey>,
    /// The partition's tree and its write records.
    partition: RwLock<Partition<E>>,
}

/// What one mutation does to one partition.
struct PartitionChange<E: RTreeObject> {
    /// The envelopes of the object's entries that leave the partition.
    leaving: Vec<E::Envelope>,
    /// The object's entries that arrive in the partition.
    arriving: Vec<E>,
}

// Manual because deriving `Default` would demand `E: Default`; two empty lists
// are what is actually wanted.
impl<E: RTreeObject> Default for PartitionChange<E> {
    fn default() -> PartitionChange<E> {
        PartitionChange {
            leaving: Vec::new(),
            arriving: Vec::new(),
        }
    }
}

/// A thread-safe R-tree partitioned by [`SpatialKey`], with a back-index for
/// scan-free removal and per-object generations for rebuild safety.
#[derive(Debug)]
pub struct PartitionedRTree<E>
where
    E: RTreeObject + Indexed,
{
    /// One tree per partition.
    buckets: DashMap<SpatialKey, Arc<Bucket<E>>>,
    /// Where each object's entries currently sit.
    placements: DashMap<<E as Indexed>::Id, Vec<Placement<E>>>,
    /// Monotonic clock stamping every mutation, so a rebuild can tell which
    /// objects have changed since its snapshot was read.
    clock: AtomicU64,
    /// Whether mutations are recorded for a rebuild to respect.
    tracking: WriteTracking,
}

impl<E> PartitionedRTree<E>
where
    E: RTreeObject + Indexed,
{
    /// An empty tree.
    #[must_use]
    pub fn new(tracking: WriteTracking) -> PartitionedRTree<E> {
        PartitionedRTree {
            buckets: DashMap::new(),
            placements: DashMap::new(),
            clock: AtomicU64::new(0),
            tracking,
        }
    }

    /// The current clock value, to be captured by a rebuild worker *before* it
    /// reads its source snapshot: any mutation applied after the capture is
    /// stamped with a later generation, and [`PartitionedRTree::rebuild`] keeps
    /// it over the snapshot.
    pub fn current_generation(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }

    /// Advance the clock and return the fresh generation to stamp a mutation.
    fn next_generation(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Replace one object's entries under one partition.
    ///
    /// An object may occupy a partition several times, so every entry it has
    /// there is passed at once: a caller looping over a single-entry insert
    /// would have each call drop the entries the previous one added. Only this
    /// partition's entries are replaced; an object indexed under several keys
    /// keeps its other entries.
    pub fn insert_all(&self, key: SpatialKey, id: <E as Indexed>::Id, entries: Vec<E>) {
        let outgoing = self.take_placements(&id, |placement| *placement.key == key);
        let installed = self.transition(&id, outgoing, vec![(key, entries)]);
        self.install_placements(id, installed);
    }

    /// Replace every entry of one object, across every partition, with
    /// `incoming`.
    ///
    /// A partition the object leaves loses its entries, a partition it joins
    /// gains them, and a partition it stays in swaps them under one lock, so no
    /// reader of any partition ever finds the object missing from a partition
    /// it belongs to both before and after the replacement.
    pub fn replace(&self, id: <E as Indexed>::Id, incoming: Vec<(SpatialKey, Vec<E>)>) {
        let outgoing = self.take_placements(&id, |_| true);
        let installed = self.transition(&id, outgoing, incoming);
        self.install_placements(id, installed);
    }

    /// Add entries for one object without removing any it already has.
    ///
    /// Used ahead of a change whose outcome is not yet known: while it is
    /// pending the object is reachable under both its old and its new
    /// geometries, which can only widen a candidate set, and the
    /// [`PartitionedRTree::replace`] that settles the change removes whichever
    /// no longer holds.
    pub fn augment(&self, id: <E as Indexed>::Id, incoming: Vec<(SpatialKey, Vec<E>)>) {
        let installed = self.transition(&id, Vec::new(), incoming);
        self.install_placements(id, installed);
    }

    /// Remove every entry belonging to an object, across every partition.
    pub fn remove<Q>(&self, id: &Q)
    where
        <E as Indexed>::Id: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some((id, outgoing)) = self.placements.remove(id) {
            self.transition(&id, outgoing, Vec::new());
        }
    }

    /// Read one partition's tree under its lock, if the partition exists.
    pub fn read_partition<R>(&self, key: &SpatialKey, read: impl FnOnce(&RTree<E>) -> R) -> Option<R> {
        let bucket = self.existing(key)?;
        let partition = bucket.partition.read();
        Some(read(&partition.tree))
    }

    /// Replace the contents of every partition `in_scope` selects with a
    /// snapshot of them, keeping whatever live mutation is newer than it.
    ///
    /// `snapshot_generation` is the clock value the rebuild worker captured
    /// before reading its source (see [`PartitionedRTree::current_generation`]).
    /// Per partition and under its lock, an object mutated live since then --
    /// inserted, moved or removed -- keeps its live state, and every other
    /// object takes the snapshot's: present if the snapshot holds it, absent
    /// otherwise. A partition the snapshot does not mention is in scope all
    /// the same and is emptied of every object the snapshot no longer holds. The
    /// rebuilt tree is bulk-loaded, a far better layout than one-at-a-time
    /// inserts, and swapped in whole, so a reader sees either the old tree or
    /// the new one.
    ///
    /// Only sound under [`WriteTracking::Recorded`]: without the records a live
    /// mutation newer than the snapshot would be overwritten by it.
    pub fn rebuild(&self, in_scope: impl Fn(&SpatialKey) -> bool, mut incoming: HashMap<SpatialKey, Vec<E>>, snapshot_generation: u64) {
        let mut keys: HashSet<SpatialKey> = self
            .buckets
            .iter()
            .filter(|bucket| in_scope(bucket.key()))
            .map(|bucket| bucket.key().clone()) // clone: the key set outlives the shard guards it is read under
            .collect();
        keys.extend(incoming.keys().filter(|key| in_scope(key)).cloned()); // clone: the incoming map keeps its keys until each partition is taken
        for key in keys {
            let snapshot = incoming.remove(&key).unwrap_or_default();
            let bucket = self.bucket(&key);
            let rebuilt = bucket.partition.write().rebuild(snapshot, snapshot_generation);
            for id in rebuilt.departed {
                self.placements.remove_if_mut(&id, |_, placements| {
                    placements.retain(|placement| *placement.key != key);
                    placements.is_empty()
                });
            }
            for (id, _) in &rebuilt.installed {
                // clone: the record is keyed by the identifier the tree's entry holds
                self.rewrite_record(id.clone(), |placements| placements.retain(|placement| *placement.key != key));
            }
            for (id, envelope) in rebuilt.installed {
                self.rewrite_record(id, |placements| {
                    placements.reserve_exact(1);
                    placements.push(Placement {
                        key: Arc::clone(&bucket.key), // clone: a reference-count bump, every placement of the partition shares its key
                        envelope,
                    });
                });
            }
        }
    }

    /// Detach an object's back-index entries that `outgoing` selects.
    fn take_placements(&self, id: &<E as Indexed>::Id, outgoing: impl Fn(&Placement<E>) -> bool) -> Vec<Placement<E>> {
        let Some(mut placements) = self.placements.get_mut(id) else {
            return Vec::new();
        };
        placements.value_mut().extract_if(.., |placement| outgoing(placement)).collect()
    }

    /// Record an object's new back-index entries.
    ///
    /// A record is held at exactly its length: almost every object has one
    /// entry, and a growth reserve on each of a million records would outweigh
    /// the entries themselves.
    fn install_placements(&self, id: <E as Indexed>::Id, installed: Vec<Placement<E>>) {
        self.rewrite_record(id, |placements| {
            if placements.is_empty() {
                *placements = installed;
            } else {
                placements.reserve_exact(installed.len());
                placements.extend(installed);
            }
        });
    }

    /// Rewrite an object's back-index record, keyed from now on by `id`, and
    /// drop the record when it is left empty.
    ///
    /// `id` is the identifier the object's newest tree entries hold. Taking it
    /// as the key keeps an identifier shared by reference count held once,
    /// rather than once by the record and again by the entries that replaced
    /// the ones the record was keyed from.
    fn rewrite_record(&self, id: <E as Indexed>::Id, rewrite: impl FnOnce(&mut Vec<Placement<E>>)) {
        match self.placements.entry(id) {
            Entry::Occupied(mut record) => {
                let mut placements = mem::take(record.get_mut());
                rewrite(&mut placements);
                if placements.is_empty() {
                    record.remove();
                } else {
                    record.replace_entry(placements);
                }
            }
            Entry::Vacant(record) => {
                let mut placements = Vec::new();
                rewrite(&mut placements);
                if !placements.is_empty() {
                    record.insert(placements);
                }
            }
        }
    }

    /// Move one object from `outgoing` to `incoming`, partition by partition,
    /// each partition under one write lock, returning the placements the
    /// incoming entries now occupy.
    fn transition(&self, id: &<E as Indexed>::Id, outgoing: Vec<Placement<E>>, incoming: Vec<(SpatialKey, Vec<E>)>) -> Vec<Placement<E>> {
        let generation = self.next_generation();
        let mut changes: HashMap<Arc<SpatialKey>, PartitionChange<E>> = HashMap::new();
        for placement in outgoing {
            changes.entry(placement.key).or_default().leaving.push(placement.envelope);
        }
        for (key, entries) in incoming {
            changes.entry(Arc::new(key)).or_default().arriving.extend(entries);
        }
        let mut installed = Vec::new();
        for (key, PartitionChange { leaving, arriving }) in changes {
            let bucket = if arriving.is_empty() {
                let Some(bucket) = self.existing(&key) else {
                    continue;
                };
                bucket
            } else {
                self.bucket(&key)
            };
            installed.reserve_exact(arriving.len());
            let mut partition = bucket.partition.write();
            for envelope in &leaving {
                partition.tree.remove_with_selection_function(EntryOf { id, envelope });
            }
            for entry in arriving {
                installed.push(Placement {
                    key: Arc::clone(&bucket.key), // clone: a reference-count bump, every placement of the partition shares its key
                    envelope: entry.envelope(),
                });
                partition.tree.insert(entry);
            }
            if self.tracking == WriteTracking::Recorded {
                partition.written.insert(id.clone(), generation); // clone: the record owns the identifier the caller lends
            }
        }
        installed
    }

    /// The partition a key names, created empty when it does not exist yet.
    fn bucket(&self, key: &SpatialKey) -> Arc<Bucket<E>> {
        self.existing(key).unwrap_or_else(|| {
            // clone: the map and the bucket each own the key; the returned handle is a reference-count bump so the shard guard is released before the partition is locked
            let bucket = self.buckets.entry(key.clone()).or_insert_with(|| {
                Arc::new(Bucket {
                    key: Arc::new(key.clone()),
                    partition: RwLock::new(Partition::default()),
                })
            });
            Arc::clone(bucket.value())
        })
    }

    /// The partition a key names, if it exists.
    fn existing(&self, key: &SpatialKey) -> Option<Arc<Bucket<E>>> {
        // clone: a reference-count bump, so the shard guard is released before the partition is locked
        self.buckets.get(key).map(|bucket| Arc::clone(bucket.value()))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        entity::entry::SpatialEntry,
        key::SpatialKey,
        partitioned_rtree::{PartitionedRTree, WriteTracking},
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, position::Position};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};
    use std::{collections::HashMap, sync::Arc};

    fn uri(raw: &str) -> Arc<Identifier> {
        Arc::new(raw.parse().expect("a well-formed URI"))
    }

    fn entry(id: &str, x: f64, y: f64) -> SpatialEntry {
        let geometry = GeometryForm::Point(Position::try_new(x, y, None).expect("a test position lies on the CRS84 axes"));
        SpatialEntry::new(uri(id), &geometry).expect("a point has a bounding rectangle")
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

    fn located(index: &PartitionedRTree<SpatialEntry>, key: &SpatialKey) -> Vec<(String, [f64; 2])> {
        let mut located = index
            .read_partition(key, |tree| {
                tree.iter()
                    .map(|entry| (entry.entity_id.to_string(), entry.envelope.lower()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        located.sort_by(|left, right| left.0.cmp(&right.0).then(left.1[0].total_cmp(&right.1[0])));
        located
    }

    #[test]
    fn inserting_an_object_under_one_key_leaves_another_key_untouched() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Unrecorded);
        index.insert_all(key_a(), uri("urn:ngsi-ld:X:1"), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)]);
        index.insert_all(key_b(), uri("urn:ngsi-ld:X:1"), vec![entry("urn:ngsi-ld:X:1", 1.0, 1.0)]);
        pretty_assertions::assert_eq!(count(&index, &key_a()), 1);
        pretty_assertions::assert_eq!(count(&index, &key_b()), 1);
    }

    #[test]
    fn removing_one_object_from_a_partition_leaves_the_rest() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Unrecorded);
        index.insert_all(key_a(), uri("urn:ngsi-ld:X:1"), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)]);
        index.insert_all(key_a(), uri("urn:ngsi-ld:X:2"), vec![entry("urn:ngsi-ld:X:2", 5.0, 5.0)]);
        index.remove(&uri("urn:ngsi-ld:X:1"));
        pretty_assertions::assert_eq!(count(&index, &key_a()), 1);
    }

    #[test]
    fn replacing_an_object_moves_it_between_partitions() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let id = uri("urn:ngsi-ld:X:1");
        index.replace(id.clone(), vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        index.replace(id, vec![(key_b(), vec![entry("urn:ngsi-ld:X:1", 1.0, 1.0)])]);
        pretty_assertions::assert_eq!(count(&index, &key_a()), 0);
        pretty_assertions::assert_eq!(located(&index, &key_b()), vec![("urn:ngsi-ld:X:1".to_owned(), [1.0, 1.0])]);
    }

    #[test]
    fn augmenting_keeps_the_old_entry_until_a_replacement_settles_it() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let id = uri("urn:ngsi-ld:X:1");
        index.replace(id.clone(), vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        index.augment(id.clone(), vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 3.0, 3.0)])]);
        pretty_assertions::assert_eq!(
            located(&index, &key_a()),
            vec![("urn:ngsi-ld:X:1".to_owned(), [0.0, 0.0]), ("urn:ngsi-ld:X:1".to_owned(), [3.0, 3.0])]
        );
        index.replace(id, vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 3.0, 3.0)])]);
        pretty_assertions::assert_eq!(located(&index, &key_a()), vec![("urn:ngsi-ld:X:1".to_owned(), [3.0, 3.0])]);
    }

    #[test]
    fn a_rebuild_keeps_a_live_insert_newer_than_its_snapshot() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let snapshot_generation = index.current_generation();
        index.replace(uri("urn:ngsi-ld:X:new"), vec![(key_a(), vec![entry("urn:ngsi-ld:X:new", 0.0, 0.0)])]);
        let snapshot = HashMap::from([(key_a(), vec![entry("urn:ngsi-ld:X:old", 9.0, 9.0)])]);
        index.rebuild(|_| true, snapshot, snapshot_generation);
        pretty_assertions::assert_eq!(
            located(&index, &key_a()),
            vec![("urn:ngsi-ld:X:new".to_owned(), [0.0, 0.0]), ("urn:ngsi-ld:X:old".to_owned(), [9.0, 9.0])]
        );
    }

    #[test]
    fn a_rebuild_keeps_a_live_move_newer_than_its_snapshot() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let id = uri("urn:ngsi-ld:X:1");
        index.replace(id.clone(), vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        let snapshot_generation = index.current_generation();
        index.replace(id, vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 4.0, 4.0)])]);
        let snapshot = HashMap::from([(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        index.rebuild(|_| true, snapshot, snapshot_generation);
        pretty_assertions::assert_eq!(located(&index, &key_a()), vec![("urn:ngsi-ld:X:1".to_owned(), [4.0, 4.0])]);
    }

    #[test]
    fn a_rebuild_keeps_a_live_removal_newer_than_its_snapshot() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let id = uri("urn:ngsi-ld:X:1");
        index.replace(id.clone(), vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        let snapshot_generation = index.current_generation();
        index.remove(&id);
        let snapshot = HashMap::from([(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        index.rebuild(|_| true, snapshot, snapshot_generation);
        pretty_assertions::assert_eq!(count(&index, &key_a()), 0);
    }

    #[test]
    fn a_rebuild_replaces_what_no_live_write_touched_since_its_snapshot() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(uri("urn:ngsi-ld:X:gone"), vec![(key_a(), vec![entry("urn:ngsi-ld:X:gone", 0.0, 0.0)])]);
        index.replace(uri("urn:ngsi-ld:X:moved"), vec![(key_b(), vec![entry("urn:ngsi-ld:X:moved", 0.0, 0.0)])]);
        let snapshot_generation = index.current_generation();
        let snapshot = HashMap::from([(key_a(), vec![entry("urn:ngsi-ld:X:moved", 7.0, 7.0)])]);
        index.rebuild(|_| true, snapshot, snapshot_generation);
        pretty_assertions::assert_eq!(located(&index, &key_a()), vec![("urn:ngsi-ld:X:moved".to_owned(), [7.0, 7.0])]);
        pretty_assertions::assert_eq!(count(&index, &key_b()), 0);
        // The departed object's back-index was pruned, so removing it is a
        // no-op, and removing the moved one leaves nothing behind.
        index.remove(&uri("urn:ngsi-ld:X:gone"));
        index.remove(&uri("urn:ngsi-ld:X:moved"));
        pretty_assertions::assert_eq!(count(&index, &key_a()), 0);
    }

    #[test]
    fn a_rebuild_leaves_partitions_outside_its_scope_alone() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(uri("urn:ngsi-ld:X:1"), vec![(key_b(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        let snapshot_generation = index.current_generation();
        let key = key_a();
        index.rebuild(|candidate| candidate == &key, HashMap::new(), snapshot_generation);
        pretty_assertions::assert_eq!(count(&index, &key_b()), 1);
    }

    #[test]
    fn a_rebuild_forgets_the_back_index_record_of_an_object_it_no_longer_holds() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(uri("urn:ngsi-ld:X:gone"), vec![(key_a(), vec![entry("urn:ngsi-ld:X:gone", 0.0, 0.0)])]);
        let snapshot_generation = index.current_generation();
        index.rebuild(|_| true, HashMap::new(), snapshot_generation);
        assert!(!index.placements.contains_key(&uri("urn:ngsi-ld:X:gone")));
    }

    #[test]
    fn removing_an_object_leaves_another_indexed_under_the_same_envelope() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Unrecorded);
        index.insert_all(key_a(), uri("urn:ngsi-ld:X:1"), vec![entry("urn:ngsi-ld:X:1", 2.0, 2.0)]);
        index.insert_all(key_a(), uri("urn:ngsi-ld:X:2"), vec![entry("urn:ngsi-ld:X:2", 2.0, 2.0)]);
        index.remove(&uri("urn:ngsi-ld:X:1"));
        pretty_assertions::assert_eq!(located(&index, &key_a()), vec![("urn:ngsi-ld:X:2".to_owned(), [2.0, 2.0])]);
    }

    #[test]
    fn replacing_an_object_removes_every_entry_it_had_under_one_envelope() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let id = uri("urn:ngsi-ld:X:1");
        index.replace(
            Arc::clone(&id),
            vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 2.0, 2.0), entry("urn:ngsi-ld:X:1", 2.0, 2.0)])],
        );
        index.replace(id, vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 5.0, 5.0)])]);
        pretty_assertions::assert_eq!(located(&index, &key_a()), vec![("urn:ngsi-ld:X:1".to_owned(), [5.0, 5.0])]);
    }

    /// A million objects with one entry each: a growth reserve on every record
    /// would cost more than the records.
    #[test]
    fn a_back_index_record_holds_exactly_its_entries() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        let snapshot_generation = index.current_generation();
        index.replace(uri("urn:ngsi-ld:X:live"), vec![(key_a(), vec![entry("urn:ngsi-ld:X:live", 0.0, 0.0)])]);
        index.rebuild(
            |_| true,
            HashMap::from([(key_b(), vec![entry("urn:ngsi-ld:X:seeded", 1.0, 1.0)])]),
            snapshot_generation,
        );
        for id in ["urn:ngsi-ld:X:live", "urn:ngsi-ld:X:seeded"] {
            let placements = index.placements.get(&uri(id)).expect("the object is indexed");
            pretty_assertions::assert_eq!((placements.len(), placements.capacity()), (1, 1), "{id}");
        }
    }

    #[test]
    fn a_rewritten_object_is_named_by_the_identifier_its_tree_entry_holds() {
        let index: PartitionedRTree<SpatialEntry> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(uri("urn:ngsi-ld:X:1"), vec![(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]);
        let snapshot_generation = index.current_generation();
        index.rebuild(
            |_| true,
            HashMap::from([(key_a(), vec![entry("urn:ngsi-ld:X:1", 0.0, 0.0)])]),
            snapshot_generation,
        );
        let held = index
            .read_partition(&key_a(), |tree| tree.iter().map(|entry| Arc::clone(&entry.entity_id)).collect::<Vec<_>>())
            .expect("the partition exists");
        let record = index.placements.get(&uri("urn:ngsi-ld:X:1")).expect("the object is indexed");
        assert!(held.iter().all(|id| Arc::ptr_eq(id, record.key())), "one allocation names the object");
    }
}
