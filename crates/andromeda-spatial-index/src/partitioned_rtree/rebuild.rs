//! Rebuilding partitions from a snapshot while their readers and writers carry
//! on.
//!
//! A partition's new tree is bulk-loaded from the snapshot with no lock held,
//! however large the partition is. Only the swap takes the partition's write
//! lock, and under it only the objects mutated since the snapshot are
//! reconciled: their snapshot entries leave the new tree, and their live
//! entries move into it from the old one, found through the back-index, which
//! a live mutation rewrites under the same lock. The lock is held for work in
//! proportion to the partition's mutations since its last rebuild, never to
//! the partition.
//!
//! The back-index is brought in step after the swap, with no lock held. A live
//! mutation of an object in between can leave an entry its record does not
//! name, which a later mutation of the object then leaves in place. That only
//! widens a candidate set, and the next rebuild drops the entry.

use crate::{
    key::SpatialKey,
    partitioned_rtree::{Bucket, Indexed, PartitionedRTree, packed_rtree::PackedRTree, partition::Partition, placement::Placement},
};
use rstar::{AABB, RTreeObject};
use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasher, RandomState},
    mem,
    sync::Arc,
};

/// The object and the lower corner of one snapshot entry.
struct Installed<Id> {
    /// The hash of `id` a partition's list of snapshot entries is ordered by.
    hash: u64,
    /// The object the entry belongs to.
    id: Id,
    /// The lower corner of the entry's box.
    corner: [f64; 2],
}

/// A partition's snapshot, loaded into a tree of its own.
struct LoadedSnapshot<E: RTreeObject + Indexed> {
    /// The snapshot's entries, bulk-loaded.
    tree: PackedRTree<E>,
    /// Every entry of `tree`, ordered by the hash of its object's identifier,
    /// so the entries of one object are found without a walk.
    installed: Vec<Installed<<E as Indexed>::Id>>,
    /// The hasher `installed` is ordered by.
    hashing: RandomState,
}

/// What a swap leaves for the back-index to settle.
struct Swapped<E: RTreeObject + Indexed> {
    /// The tree swapped out, still holding every entry of each object not
    /// mutated since the snapshot.
    old: PackedRTree<E>,
    /// The objects mutated since the snapshot, whose live entries the
    /// partition keeps.
    live: HashSet<<E as Indexed>::Id>,
    /// The snapshot entries the partition now holds.
    installed: Vec<Installed<<E as Indexed>::Id>>,
}

impl<E: RTreeObject<Envelope = AABB<[f64; 2]>> + Indexed> LoadedSnapshot<E> {
    /// Bulk-load `snapshot` into a tree of its own.
    fn load(snapshot: Vec<E>) -> LoadedSnapshot<E> {
        let hashing = RandomState::new();
        let mut installed: Vec<Installed<<E as Indexed>::Id>> = snapshot
            .iter()
            .map(|entry| Installed {
                hash: hashing.hash_one(entry.id()),
                id: entry.id().clone(), // clone: the entries move into the tree, so the list owns each identifier
                corner: entry.envelope().lower(),
            })
            .collect();
        installed.sort_unstable_by_key(|entry| entry.hash);
        LoadedSnapshot {
            tree: PackedRTree::bulk_load(snapshot),
            installed,
            hashing,
        }
    }
}

impl<E> PartitionedRTree<E>
where
    E: RTreeObject<Envelope = AABB<[f64; 2]>> + Indexed,
{
    /// Replace the contents of every partition `in_scope` selects with a
    /// snapshot of them, keeping whatever live mutation is newer than it.
    ///
    /// `snapshot_generation` is the clock value the rebuild worker captured
    /// before reading its source (see [`PartitionedRTree::current_generation`]).
    /// An object mutated live since then (inserted, moved or removed) keeps its
    /// live state, and every other object takes the snapshot's: present if the
    /// snapshot holds it, absent otherwise. A partition the snapshot does not
    /// mention is in scope all the same and is emptied of every object the
    /// snapshot no longer holds.
    ///
    /// Each partition's new tree is bulk-loaded, a far better layout than
    /// one-at-a-time inserts, with no lock held, and swapped in whole, so a
    /// reader sees either the old tree or the new one. The partition's write
    /// lock is held for the swap and for moving the entries of the objects
    /// mutated since the snapshot.
    ///
    /// Only sound under [`WriteTracking::Recorded`](crate::partitioned_rtree::WriteTracking::Recorded):
    /// without the records a live mutation newer than the snapshot would be
    /// overwritten by it.
    pub fn rebuild(&self, in_scope: impl Fn(&SpatialKey) -> bool, mut incoming: HashMap<SpatialKey, Vec<E>>, snapshot_generation: u64) {
        let mut keys: HashSet<SpatialKey> = self
            .buckets
            .iter()
            .filter(|bucket| in_scope(bucket.key()))
            .map(|bucket| bucket.key().clone()) // clone: the key set outlives the shard guards it is read under
            .collect();
        keys.extend(incoming.keys().filter(|key| in_scope(key)).cloned()); // clone: the incoming map keeps its keys until each partition is taken
        for key in keys {
            let snapshot = LoadedSnapshot::load(incoming.remove(&key).unwrap_or_default());
            let bucket = self.bucket(&key);
            let swapped = self.swap(&bucket, snapshot, snapshot_generation);
            self.settle(&bucket.key, swapped);
        }
    }

    /// Swap `snapshot` in for the partition's tree under its write lock,
    /// keeping the live entries of every object mutated after
    /// `snapshot_generation`.
    ///
    /// A live object's entries are moved by the corners its record names. Every
    /// entry of the object at such a corner moves, so an entry the record lost
    /// track of can only come along, never be moved in place of the one it
    /// names.
    fn swap(&self, bucket: &Bucket<E>, snapshot: LoadedSnapshot<E>, snapshot_generation: u64) -> Swapped<E> {
        let LoadedSnapshot {
            mut tree,
            mut installed,
            hashing,
        } = snapshot;
        let mut withdrawn: Vec<usize> = Vec::new();
        let mut partition = bucket.partition.write();
        let Partition { tree: current, written } = &mut *partition;
        let newer: HashMap<<E as Indexed>::Id, u64> = written.extract_if(|_, generation| *generation > snapshot_generation).collect();
        let superseded = mem::replace(written, newer);
        // clone: the live objects are settled after the lock is released
        let live: HashSet<<E as Indexed>::Id> = written.keys().cloned().collect();
        for id in &live {
            let hash = hashing.hash_one(id);
            let start = installed.partition_point(|entry| entry.hash < hash);
            for (offset, entry) in installed[start..].iter().take_while(|entry| entry.hash == hash).enumerate() {
                if entry.id == *id {
                    tree.remove(id, entry.corner);
                    withdrawn.push(start + offset);
                }
            }
            let Some(record) = self.placements.get(id) else {
                continue;
            };
            for placement in record.value().iter().filter(|placement| placement.key == bucket.key) {
                while let Some(entry) = current.remove(id, placement.corner) {
                    tree.insert(entry);
                }
            }
        }
        let old = mem::replace(current, tree);
        drop(partition);
        drop(superseded);
        // Highest first, so a swap only ever moves an entry that stays.
        withdrawn.sort_unstable_by(|left, right| right.cmp(left));
        for position in withdrawn {
            installed.swap_remove(position);
        }
        Swapped { old, live, installed }
    }

    /// Bring the back-index in step with a swapped partition.
    ///
    /// Every object the old tree still holds loses its placements in the
    /// partition, then every snapshot entry the partition now holds is
    /// recorded, so an object the snapshot dropped is forgotten and one it
    /// holds is placed where the snapshot put it. A live object's record is
    /// left as its own mutations wrote it.
    fn settle(&self, key: &Arc<SpatialKey>, swapped: Swapped<E>) {
        let Swapped { old, live, installed } = swapped;
        for entry in old.into_entries().filter(|entry| !live.contains(entry.id())) {
            self.placements.remove_if_mut(entry.id(), |_, placements| {
                placements.retain(|placement| placement.key != *key);
                placements.is_empty()
            });
        }
        for Installed { id, corner, .. } in installed {
            self.rewrite_record(id, |_, placements| {
                placements.push(Placement { key: Arc::clone(key), corner });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        key::SpatialKey,
        partitioned_rtree::{
            Indexed,
            PartitionedRTree,
            WriteTracking,
            placement::{Placement, Placements},
        },
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};
    use pretty_assertions::assert_eq;
    use rstar::{AABB, RTreeObject};
    use std::{
        collections::HashMap,
        sync::{Arc, Condvar, Mutex, mpsc},
        thread,
        time::Duration,
    };

    /// How far a [`Gate`] has come.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum GateState {
        /// No thread has reached the gate.
        Shut,
        /// A thread waits at the gate.
        Reached,
        /// Every thread passes.
        Open,
    }

    /// Holds every thread that reaches it until the test opens it.
    #[derive(Debug)]
    struct Gate {
        /// How far the gate has come.
        state: Mutex<GateState>,
        /// Signalled on every change of `state`.
        changed: Condvar,
    }

    impl Gate {
        fn shut() -> Arc<Gate> {
            Arc::new(Gate {
                state: Mutex::new(GateState::Shut),
                changed: Condvar::new(),
            })
        }

        fn pass(&self) {
            let mut state = self.state.lock().expect("no gate user panics");
            if *state == GateState::Shut {
                *state = GateState::Reached;
                self.changed.notify_all();
            }
            while *state != GateState::Open {
                state = self.changed.wait(state).expect("no gate user panics");
            }
        }

        fn reached(&self) {
            let mut state = self.state.lock().expect("no gate user panics");
            while *state == GateState::Shut {
                state = self.changed.wait(state).expect("no gate user panics");
            }
        }

        fn open(&self) {
            *self.state.lock().expect("no gate user panics") = GateState::Open;
            self.changed.notify_all();
        }
    }

    /// A point named by a number, whose box is read only once its gate, if it
    /// has one, is open.
    #[derive(Debug)]
    struct Marker {
        /// The number naming the point.
        id: u32,
        /// Where the point lies.
        at: [f64; 2],
        /// The gate reading the box waits at.
        gate: Option<Arc<Gate>>,
    }

    impl RTreeObject for Marker {
        type Envelope = AABB<[f64; 2]>;

        fn envelope(&self) -> AABB<[f64; 2]> {
            if let Some(gate) = &self.gate {
                gate.pass();
            }
            AABB::from_point(self.at)
        }
    }

    impl Indexed for Marker {
        type Id = u32;

        fn id(&self) -> &u32 {
            &self.id
        }
    }

    fn marker(id: u32, longitude: f64) -> Marker {
        Marker {
            id,
            at: [longitude, 0.0],
            gate: None,
        }
    }

    fn gated(id: u32, longitude: f64, gate: &Arc<Gate>) -> Marker {
        Marker {
            id,
            at: [longitude, 0.0],
            gate: Some(Arc::clone(gate)),
        }
    }

    fn key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/A")),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        )
    }

    fn located(index: &PartitionedRTree<Marker>) -> Vec<(u32, f64)> {
        let mut located = index
            .read_partition(&key(), |tree| tree.iter().map(|entry| (entry.id, entry.at[0])).collect::<Vec<_>>())
            .unwrap_or_default();
        located.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.total_cmp(&right.1)));
        located
    }

    fn record(index: &PartitionedRTree<Marker>, id: u32) -> Option<Vec<f64>> {
        index
            .placements
            .get(&id)
            .map(|record| record.value().iter().map(|placement| placement.corner[0]).collect())
    }

    /// Rebuild `index` from `snapshot`, holding the rebuild where it reads the
    /// box of the snapshot's gated entry while `meanwhile` runs on a thread of
    /// its own, and return what `meanwhile` returned, or `None` when it did not
    /// return within ten seconds.
    fn while_loading<R: Send>(
        index: &PartitionedRTree<Marker>,
        gate: &Gate,
        snapshot: Vec<Marker>,
        snapshot_generation: u64,
        meanwhile: impl FnOnce() -> R + Send,
    ) -> Option<R> {
        thread::scope(|scope| {
            let rebuild = scope.spawn(move || index.rebuild(|_| true, HashMap::from([(key(), snapshot)]), snapshot_generation));
            gate.reached();
            let (done, finished) = mpsc::channel();
            scope.spawn(move || done.send(meanwhile()));
            let returned = finished.recv_timeout(Duration::from_secs(10)).ok();
            gate.open();
            rebuild.join().expect("the rebuild completes");
            returned
        })
    }

    #[test]
    fn a_partition_is_read_and_written_while_its_rebuild_loads_the_snapshot() {
        let index: PartitionedRTree<Marker> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(1, vec![(key(), vec![marker(1, 1.0)])]);
        let snapshot_generation = index.current_generation();
        let gate = Gate::shut();

        let seen = while_loading(&index, &gate, vec![marker(1, 1.0), gated(2, 2.0, &gate)], snapshot_generation, || {
            index.replace(3, vec![(key(), vec![marker(3, 3.0)])]);
            located(&index)
        });

        assert_eq!(seen, Some(vec![(1, 1.0), (3, 3.0)]), "the old tree answers until the swap");
        assert_eq!(located(&index), vec![(1, 1.0), (2, 2.0), (3, 3.0)]);
        assert_eq!(record(&index, 3), Some(vec![3.0]));
    }

    #[test]
    fn an_object_moved_while_its_rebuild_loads_the_snapshot_is_only_at_its_new_place() {
        let index: PartitionedRTree<Marker> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(1, vec![(key(), vec![marker(1, 1.0)])]);
        let snapshot_generation = index.current_generation();
        let gate = Gate::shut();

        let moved = while_loading(&index, &gate, vec![marker(1, 1.0), gated(2, 2.0, &gate)], snapshot_generation, || {
            index.replace(1, vec![(key(), vec![marker(1, 5.0)])]);
        });

        assert_eq!(moved, Some(()));
        assert_eq!(located(&index), vec![(1, 5.0), (2, 2.0)]);
        assert_eq!(
            index.placements.get(&1).map(|record| record.value().iter().count()),
            Some(1),
            "one placement, held in place"
        );
        assert_eq!(record(&index, 1), Some(vec![5.0]));
        index.remove(&1);
        assert_eq!(located(&index), vec![(2, 2.0)]);
    }

    #[test]
    fn an_object_removed_while_its_rebuild_loads_the_snapshot_stays_removed() {
        let index: PartitionedRTree<Marker> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(1, vec![(key(), vec![marker(1, 1.0)])]);
        let snapshot_generation = index.current_generation();
        let gate = Gate::shut();

        let removed = while_loading(&index, &gate, vec![marker(1, 1.0), gated(2, 2.0, &gate)], snapshot_generation, || {
            index.remove(&1);
        });

        assert_eq!(removed, Some(()));
        assert_eq!(located(&index), vec![(2, 2.0)]);
        assert_eq!(record(&index, 1), None);
    }

    #[test]
    fn an_object_the_snapshot_moved_is_recorded_at_the_snapshot_place() {
        let index: PartitionedRTree<Marker> = PartitionedRTree::new(WriteTracking::Recorded);
        index.replace(1, vec![(key(), vec![marker(1, 1.0)])]);
        let snapshot_generation = index.current_generation();

        index.rebuild(|_| true, HashMap::from([(key(), vec![marker(1, 4.0)])]), snapshot_generation);

        assert_eq!(located(&index), vec![(1, 4.0)]);
        assert_eq!(
            index.placements.get(&1).map(|record| *record.value()
                == Placements::One(Placement {
                    key: Arc::new(key()),
                    corner: [4.0, 0.0],
                })),
            Some(true)
        );
        index.replace(1, vec![(key(), vec![marker(1, 6.0)])]);
        assert_eq!(located(&index), vec![(1, 6.0)]);
    }

    #[test]
    fn every_live_entry_of_an_object_at_one_corner_moves_into_the_rebuilt_tree() {
        let index: PartitionedRTree<Marker> = PartitionedRTree::new(WriteTracking::Recorded);
        let snapshot_generation = index.current_generation();
        index.replace(1, vec![(key(), vec![marker(1, 1.0), marker(1, 1.0)])]);

        index.rebuild(|_| true, HashMap::from([(key(), vec![marker(1, 9.0), marker(2, 2.0)])]), snapshot_generation);

        assert_eq!(located(&index), vec![(1, 1.0), (1, 1.0), (2, 2.0)]);
        assert_eq!(record(&index, 1), Some(vec![1.0, 1.0]));
    }
}
