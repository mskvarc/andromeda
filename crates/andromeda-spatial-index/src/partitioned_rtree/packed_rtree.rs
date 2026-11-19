//! An R-tree whose leaves pack their entries.
//!
//! An `rstar` tree keeps each object in a node that can also be a parent, a
//! list of children and their box, so an object takes that node's size however
//! small it is: 56 bytes for a planar box, where a point entry needs 24. Here
//! the `rstar` tree holds leaves instead, each a box and up to sixteen entries
//! in one allocation of exactly their length, so an entry costs its own size
//! and a sixteenth of its leaf's node.
//!
//! Two invariants hold between operations: every leaf holds at least one entry
//! and at most sixteen, and a leaf's box is the merge of its entries' boxes. A
//! search tests each entry's own box before handing the entry out, so it finds
//! exactly the entries whose box meets the region, as a tree of single entries
//! would.

use crate::partitioned_rtree::Indexed;
use rstar::{AABB, Envelope, Point, PointDistance, RTree, RTreeObject, SelectionFunction};
use std::{iter, mem};

/// The most entries one leaf holds.
///
/// A leaf's node is shared by its entries, under 4 bytes each at sixteen, and
/// a search that meets a leaf tests at most sixteen boxes.
const LEAF_CAPACITY: usize = 16;

/// The axis of a planar box that runs along longitude.
const LONGITUDE: usize = 0;

/// The axis of a planar box that runs along latitude.
const LATITUDE: usize = 1;

/// An R-tree of entries packed into leaves of up to sixteen.
#[derive(Debug)]
pub struct PackedRTree<E: RTreeObject> {
    /// The leaves, indexed by their boxes.
    leaves: RTree<Leaf<E>>,
}

/// Up to sixteen entries and the box covering them.
#[derive(Debug)]
struct Leaf<E: RTreeObject> {
    /// The merge of the entries' boxes.
    envelope: E::Envelope,
    /// The entries, in one allocation of exactly their length.
    entries: Box<[E]>,
}

// Manual because deriving `Default` would demand `E: Default`; an empty tree is
// what is actually wanted.
impl<E: RTreeObject> Default for PackedRTree<E> {
    fn default() -> PackedRTree<E> {
        PackedRTree { leaves: RTree::new() }
    }
}

impl<E: RTreeObject> PackedRTree<E> {
    /// Every entry of the tree.
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.leaves.iter().flat_map(|leaf| leaf.entries.iter())
    }

    /// Every entry of the tree, taken out of it.
    pub fn into_entries(self) -> impl Iterator<Item = E> {
        self.leaves.into_iter().flat_map(|leaf| leaf.entries)
    }
}

impl<E: RTreeObject<Envelope = AABB<[f64; 2]>>> PackedRTree<E> {
    /// A tree of `entries`, packed sort-tile-recursive: sorted by longitude
    /// into vertical slabs a whole number of leaves long, each slab sorted by
    /// latitude and cut into full leaves, so a leaf covers entries that lie
    /// together and only the last leaf has room.
    #[must_use]
    pub fn bulk_load(mut entries: Vec<E>) -> PackedRTree<E> {
        let leaves = entries.len().div_ceil(LEAF_CAPACITY);
        let slabs = leaves.isqrt() + usize::from(leaves.isqrt().pow(2) < leaves);
        let slab_length = leaves.div_ceil(slabs.max(1)) * LEAF_CAPACITY;
        entries.sort_unstable_by(|left, right| midpoint(left, LONGITUDE).total_cmp(&midpoint(right, LONGITUDE)));
        for slab in entries.chunks_mut(slab_length.max(LEAF_CAPACITY)) {
            slab.sort_unstable_by(|left, right| midpoint(left, LATITUDE).total_cmp(&midpoint(right, LATITUDE)));
        }
        let mut entries = entries.into_iter();
        let leaves = iter::from_fn(|| {
            let packed: Box<[E]> = entries.by_ref().take(LEAF_CAPACITY).collect();
            (!packed.is_empty()).then(|| Leaf::of(packed))
        })
        .collect();
        PackedRTree {
            leaves: RTree::bulk_load(leaves),
        }
    }

    /// Add one entry.
    ///
    /// A leaf whose box already covers the entry and that has room takes it in
    /// place: the box stays as it is, so the tree above the leaf stays valid.
    /// Otherwise the leaf nearest the entry leaves the tree, takes the entry,
    /// and returns under the grown box, split in two once it holds more than
    /// sixteen.
    pub fn insert(&mut self, entry: E) {
        let envelope = entry.envelope();
        if let Some(leaf) = self
            .leaves
            .locate_in_envelope_intersecting_mut(envelope)
            .find(|leaf| leaf.entries.len() < LEAF_CAPACITY && leaf.envelope.contains_envelope(&envelope))
        {
            leaf.push(entry);
            return;
        }
        let grown = match self.leaves.pop_nearest_neighbor(envelope.center()) {
            Some(nearest) => nearest.with(entry),
            None => Leaf::of(Box::new([entry])),
        };
        if grown.entries.len() > LEAF_CAPACITY {
            let [lower, upper] = grown.split();
            self.leaves.insert(lower);
            self.leaves.insert(upper);
        } else {
            self.leaves.insert(grown);
        }
    }

    /// The entries whose box meets `envelope`.
    pub fn intersecting(&self, envelope: AABB<[f64; 2]>) -> impl Iterator<Item = &E> {
        self.leaves
            .locate_in_envelope_intersecting(envelope)
            .flat_map(move |leaf| leaf.entries.iter().filter(move |entry| entry.envelope().intersects(&envelope)))
    }
}

impl<E: RTreeObject<Envelope = AABB<[f64; 2]>> + Indexed> PackedRTree<E> {
    /// Take out one entry of the object `id` whose box has its lower corner at
    /// `corner`, if the tree holds one.
    ///
    /// The entry's leaf leaves the tree and returns under the box of the
    /// entries it keeps, or stays out once it keeps none.
    pub fn remove(&mut self, id: &<E as Indexed>::Id, corner: [f64; 2]) -> Option<E> {
        let leaf = self.leaves.remove_with_selection_function(LeafHolding { id, corner })?;
        let mut kept = leaf.entries.into_vec();
        let removed = kept
            .iter()
            .position(|entry| starts_at(entry, id, corner))
            .map(|position| kept.swap_remove(position));
        if !kept.is_empty() {
            self.leaves.insert(Leaf::of(kept.into_boxed_slice()));
        }
        removed
    }
}

impl<E: RTreeObject> Leaf<E> {
    /// A leaf of `entries` under the merge of their boxes.
    fn of(entries: Box<[E]>) -> Leaf<E> {
        let envelope = entries.iter().fold(E::Envelope::new_empty(), |merged, entry| merged.merged(&entry.envelope()));
        Leaf { envelope, entries }
    }

    /// Add an entry the box already covers.
    fn push(&mut self, entry: E) {
        let mut entries = mem::take(&mut self.entries).into_vec();
        entries.reserve_exact(1);
        entries.push(entry);
        self.entries = entries.into_boxed_slice();
    }

    /// This leaf with one more entry, under its box grown to cover it.
    fn with(mut self, entry: E) -> Leaf<E> {
        self.envelope.merge(&entry.envelope());
        self.push(entry);
        self
    }
}

impl<E: RTreeObject<Envelope = AABB<[f64; 2]>>> Leaf<E> {
    /// The leaf's entries as two leaves of half each, cut across the axis its
    /// box is longest along, so the two overlap little.
    fn split(self) -> [Leaf<E>; 2] {
        let extent = |axis: usize| self.envelope.upper()[axis] - self.envelope.lower()[axis];
        let axis = if extent(LONGITUDE) >= extent(LATITUDE) { LONGITUDE } else { LATITUDE };
        let mut lower = self.entries.into_vec();
        lower.sort_unstable_by(|left, right| midpoint(left, axis).total_cmp(&midpoint(right, axis)));
        let upper = lower.split_off(lower.len() / 2);
        [Leaf::of(lower.into_boxed_slice()), Leaf::of(upper.into_boxed_slice())]
    }
}

impl<E: RTreeObject> RTreeObject for Leaf<E> {
    type Envelope = E::Envelope;

    fn envelope(&self) -> E::Envelope {
        self.envelope.clone()
    }
}

// An insertion that no leaf covers grows the leaf nearest it, and a leaf is as
// near as its box.
impl<E: RTreeObject> PointDistance for Leaf<E> {
    fn distance_2(&self, point: &<E::Envelope as Envelope>::Point) -> <<E::Envelope as Envelope>::Point as Point>::Scalar {
        self.envelope.distance_2(point)
    }
}

/// The middle of an entry's box along one axis.
fn midpoint<E: RTreeObject<Envelope = AABB<[f64; 2]>>>(entry: &E, axis: usize) -> f64 {
    entry.envelope().center()[axis]
}

/// Whether `entry` belongs to the object `id` and its box has its lower corner
/// at `corner`.
///
/// The corner is read off the entry itself when the entry is placed, so the
/// entry's own corner matches it bit for bit, and no tolerance is wanted.
fn starts_at<E: RTreeObject<Envelope = AABB<[f64; 2]>> + Indexed>(entry: &E, id: &<E as Indexed>::Id, corner: [f64; 2]) -> bool {
    entry.id() == id && entry.envelope().lower().map(f64::to_bits) == corner.map(f64::to_bits)
}

/// Selects the leaf holding an entry of one object by the lower corner of the
/// entry's box.
///
/// Every box on the way down to the entry holds that corner. Two entries of an
/// object that share a partition and a corner are interchangeable here: an
/// object's entries in a partition always leave together, so whichever is
/// found first is one of the leaving ones.
struct LeafHolding<'a, E: Indexed> {
    /// The object the entry belongs to.
    id: &'a <E as Indexed>::Id,
    /// The lower corner of the entry's box.
    corner: [f64; 2],
}

impl<E: RTreeObject<Envelope = AABB<[f64; 2]>> + Indexed> SelectionFunction<Leaf<E>> for LeafHolding<'_, E> {
    fn should_unpack_parent(&self, envelope: &AABB<[f64; 2]>) -> bool {
        envelope.contains_point(&self.corner)
    }

    fn should_unpack_leaf(&self, leaf: &Leaf<E>) -> bool {
        leaf.envelope.contains_point(&self.corner) && leaf.entries.iter().any(|entry| starts_at(entry, self.id, self.corner))
    }
}

#[cfg(test)]
mod tests {
    use crate::partitioned_rtree::{
        Indexed,
        packed_rtree::{LEAF_CAPACITY, Leaf, PackedRTree},
    };
    use pretty_assertions::assert_eq;
    use rstar::{AABB, Envelope, RTreeObject};

    /// A box named by a number.
    #[derive(Debug, Clone, PartialEq)]
    struct Marker {
        /// The number naming the box.
        id: u32,
        /// The box, a point when its corners meet.
        envelope: AABB<[f64; 2]>,
    }

    impl RTreeObject for Marker {
        type Envelope = AABB<[f64; 2]>;

        fn envelope(&self) -> AABB<[f64; 2]> {
            self.envelope
        }
    }

    impl Indexed for Marker {
        type Id = u32;

        fn id(&self) -> &u32 {
            &self.id
        }
    }

    /// Marker `id`, scattered over the globe by its number: a point, or a box
    /// up to a degree and a half wide.
    fn marker(id: u32) -> Marker {
        let lower = [f64::from(id * 37 % 360) - 180.0, f64::from(id * 53 % 170) - 85.0];
        let width = f64::from(id % 4) * 0.5;
        Marker {
            id,
            envelope: AABB::from_corners(lower, [lower[0] + width, lower[1] + width]),
        }
    }

    fn at(id: u32, longitude: f64, latitude: f64) -> Marker {
        Marker {
            id,
            envelope: AABB::from_point([longitude, latitude]),
        }
    }

    fn leaves(tree: &PackedRTree<Marker>) -> Vec<&Leaf<Marker>> {
        tree.leaves.iter().collect()
    }

    /// Every leaf holds one to sixteen entries under the merge of their boxes.
    fn assert_packed(tree: &PackedRTree<Marker>) {
        for leaf in leaves(tree) {
            assert!((1..=LEAF_CAPACITY).contains(&leaf.entries.len()), "{} entries", leaf.entries.len());
            let merged = leaf
                .entries
                .iter()
                .fold(AABB::new_empty(), |merged: AABB<[f64; 2]>, entry| merged.merged(&entry.envelope()));
            assert_eq!(leaf.envelope, merged);
        }
    }

    fn ids<'t>(entries: impl Iterator<Item = &'t Marker>) -> Vec<u32> {
        let mut ids: Vec<u32> = entries.map(|entry| entry.id).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn a_bulk_load_fills_every_leaf_but_the_last() {
        let tree = PackedRTree::bulk_load((0..1000).map(marker).collect());
        assert_packed(&tree);
        assert_eq!(leaves(&tree).len(), 63);
        assert_eq!(leaves(&tree).iter().filter(|leaf| leaf.entries.len() < LEAF_CAPACITY).count(), 1);
        assert_eq!(ids(tree.iter()), (0..1000).collect::<Vec<_>>());
    }

    #[test]
    fn a_search_finds_exactly_the_entries_whose_box_meets_it_through_loads_inserts_and_removals() {
        let mut tree = PackedRTree::bulk_load((0..1000).map(marker).collect());
        let mut held: Vec<Marker> = (0..1000).map(marker).collect();
        for gone in held.iter().filter(|entry| entry.id % 3 == 0) {
            tree.remove(&gone.id, gone.envelope.lower());
        }
        held.retain(|entry| entry.id % 3 != 0);
        for id in 1000..1500 {
            tree.insert(marker(id));
            held.push(marker(id));
        }
        assert_packed(&tree);
        let regions = [
            AABB::from_corners([-180.0, -90.0], [180.0, 90.0]),
            AABB::from_corners([-10.0, -10.0], [10.0, 10.0]),
            AABB::from_corners([170.0, -85.0], [180.0, 0.0]),
            AABB::from_corners([-180.0, 40.0], [-120.0, 85.0]),
            AABB::from_point([-143.0, -32.0]),
            AABB::from_corners([0.0, 86.0], [10.0, 90.0]),
        ];
        for region in regions {
            assert_eq!(
                ids(tree.intersecting(region)),
                ids(held.iter().filter(|entry| entry.envelope().intersects(&region))),
                "{region:?}"
            );
        }
        assert_eq!(ids(tree.iter()), ids(held.iter()));
    }

    #[test]
    fn a_full_leaf_splits_in_two_on_one_more_entry() {
        let mut tree = PackedRTree::default();
        for id in 0..=16 {
            tree.insert(at(id, f64::from(id) * 0.001, 0.0));
        }
        assert_packed(&tree);
        assert_eq!(leaves(&tree).iter().map(|leaf| leaf.entries.len()).sum::<usize>(), 17);
        assert_eq!(leaves(&tree).len(), 2);
    }

    #[test]
    fn an_entry_inside_a_leaf_with_room_joins_it_in_place() {
        let mut tree = PackedRTree::default();
        tree.insert(at(1, 0.0, 0.0));
        tree.insert(at(2, 2.0, 2.0));
        tree.insert(at(3, 1.0, 1.0));
        assert_packed(&tree);
        assert_eq!(leaves(&tree).len(), 1);
        assert_eq!(leaves(&tree)[0].envelope, AABB::from_corners([0.0, 0.0], [2.0, 2.0]));
    }

    #[test]
    fn removing_an_entry_shrinks_its_leaf_to_the_entries_it_keeps() {
        let mut tree = PackedRTree::default();
        tree.insert(at(1, 0.0, 0.0));
        tree.insert(at(2, 5.0, 5.0));
        tree.remove(&2, [5.0, 5.0]);
        assert_eq!(leaves(&tree).len(), 1);
        assert_eq!(leaves(&tree)[0].envelope, AABB::from_point([0.0, 0.0]));
    }

    #[test]
    fn a_removed_entry_is_handed_back_and_a_missing_one_is_not() {
        let mut tree = PackedRTree::default();
        tree.insert(at(1, 3.0, 4.0));
        assert_eq!(tree.remove(&1, [4.0, 3.0]), None);
        assert_eq!(tree.remove(&1, [3.0, 4.0]), Some(at(1, 3.0, 4.0)));
        assert_eq!(tree.remove(&1, [3.0, 4.0]), None);
    }

    #[test]
    fn removing_the_last_entry_of_a_leaf_drops_the_leaf() {
        let mut tree = PackedRTree::default();
        tree.insert(at(1, 3.0, 4.0));
        tree.remove(&1, [3.0, 4.0]);
        assert_eq!(leaves(&tree).len(), 0);
    }

    #[test]
    fn removing_an_entry_at_another_corner_leaves_the_tree_as_it_is() {
        let mut tree = PackedRTree::default();
        tree.insert(at(1, 3.0, 4.0));
        tree.insert(Marker {
            id: 2,
            envelope: AABB::from_corners([1.0, 1.0], [5.0, 5.0]),
        });
        tree.remove(&1, [4.0, 3.0]);
        tree.remove(&2, [3.0, 4.0]);
        tree.remove(&2, [5.0, 5.0]);
        assert_eq!(ids(tree.iter()), vec![1, 2]);
    }

    #[test]
    fn a_box_is_removed_by_its_lower_corner() {
        let mut tree = PackedRTree::default();
        tree.insert(Marker {
            id: 1,
            envelope: AABB::from_corners([1.0, 1.0], [5.0, 5.0]),
        });
        tree.insert(at(2, 1.0, 1.0));
        tree.remove(&1, [1.0, 1.0]);
        assert_eq!(ids(tree.iter()), vec![2]);
        assert_eq!(leaves(&tree)[0].envelope, AABB::from_point([1.0, 1.0]));
    }

    #[test]
    fn entries_of_one_object_at_one_corner_leave_one_at_a_time() {
        let mut tree = PackedRTree::default();
        tree.insert(at(1, 3.0, 4.0));
        tree.insert(Marker {
            id: 1,
            envelope: AABB::from_corners([3.0, 4.0], [6.0, 8.0]),
        });
        tree.remove(&1, [3.0, 4.0]);
        assert_eq!(ids(tree.iter()), vec![1]);
        tree.remove(&1, [3.0, 4.0]);
        assert_eq!(ids(tree.iter()), Vec::<u32>::new());
    }
}
