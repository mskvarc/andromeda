//! The R-tree of entity geometries.

use crate::{entity::entry::SpatialEntry, key::SpatialKey};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, relation::matches_geo};
use dashmap::DashMap;
use geo::algorithm::BoundingRect;
use parking_lot::RwLock;
use rstar::{AABB, RTree};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use urn_rs::Urn;

/// One partition's R-tree, tagged with the generation at which it was last
/// modified.
///
/// The generation is what lets a bulk rebuild refuse to overwrite a partition
/// that a live write has touched since the rebuild's snapshot was taken: the
/// index is only ever an optimisation, so it must never lose the newer of two
/// writes to the tree.
#[derive(Debug)]
struct Partition {
    /// The [`SpatialEntityIndex`] clock value at this partition's most recent
    /// mutation.
    generation: u64,
    /// The R-tree of entity geometries in this partition.
    tree: RTree<SpatialEntry>,
}

/// Thread-safe R-tree cache of entity geometries.
///
/// Each `(tenant, type, geoproperty)` partition has its own tree behind its
/// own lock, so a write to one tenant does not stall a read of another. A
/// secondary map records every partition an entity was last indexed under, so
/// removal is proportional to that entity's geoproperties rather than to the
/// number of partitions.
#[derive(Debug, Default)]
pub struct SpatialEntityIndex {
    /// One tree per partition.
    buckets: DashMap<SpatialKey, Arc<RwLock<Partition>>>,
    /// Which partitions each entity currently appears in.
    by_entity: DashMap<Urn, Vec<SpatialKey>>,
    /// Monotonic clock stamping every mutation, so a bulk rebuild can tell
    /// whether a partition has changed since its snapshot was taken.
    clock: AtomicU64,
}

impl SpatialEntityIndex {
    /// An empty index.
    pub fn new() -> SpatialEntityIndex {
        SpatialEntityIndex::default()
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

    /// Evaluate a `georel` filter against one partition.
    ///
    /// `None` means the partition is cold, which is not the same answer as an
    /// empty set: the caller falls through to the store rather than reporting
    /// that nothing matched.
    pub fn query(&self, key: &SpatialKey, georel: &GeoRel, reference: &geo::Geometry<f64>) -> Option<HashSet<Urn>> {
        let bucket = self.buckets.get(key)?;
        let partition = bucket.value().read();
        Some(evaluate(&partition.tree, georel, reference))
    }

    /// Insert or overwrite the entry for one entity under one partition.
    pub fn insert(&self, key: SpatialKey, entity_id: Urn, geometry: Geometry) {
        let Some(entry) = SpatialEntry::new(entity_id.clone(), geometry) else {
            // clone: the entry owns its identifier and the caller keeps one
            return;
        };

        self.remove_from_key(&entity_id, &key);

        let generation = self.next_generation();
        {
            let bucket = self
                .buckets
                .entry(key.clone()) // clone: the bucket key and the ownership record are separate
                .or_insert_with(|| Arc::new(RwLock::new(Partition { generation, tree: RTree::new() })));
            let mut partition = bucket.value().write();
            partition.tree.insert(entry);
            partition.generation = generation;
        }

        let mut owned_keys = self.by_entity.entry(entity_id).or_default();
        if !owned_keys.value().contains(&key) {
            owned_keys.value_mut().push(key);
        }
    }

    /// Remove every entry belonging to an entity, across every partition.
    pub fn remove(&self, entity_id: &Urn) {
        let Some((_, keys)) = self.by_entity.remove(entity_id) else {
            return;
        };
        for key in keys {
            self.remove_from_bucket(entity_id, &key);
        }
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
    /// state, and any entity that was in the partition but not in the new
    /// snapshot is pruned from `by_entity` so its ownership record cannot leak.
    ///
    /// [`current_generation`]: SpatialEntityIndex::current_generation
    pub fn bulk_load(&self, key: SpatialKey, entries: Vec<SpatialEntry>, snapshot_generation: u64) {
        // clone: the ownership records outlive the entry list, which the tree consumes
        let incoming: HashSet<Urn> = entries.iter().map(|entry| entry.entity_id.clone()).collect();
        let tree = RTree::bulk_load(entries);

        let departed: Vec<Urn> = {
            let bucket = self.buckets.entry(key.clone()).or_insert_with(|| Arc::new(RwLock::new(Partition { generation: 0, tree: RTree::new() })));
            let mut partition = bucket.value().write();
            if partition.generation > snapshot_generation {
                // A write newer than the snapshot already changed this
                // partition; keeping it is what makes the index only ever an
                // optimisation rather than a source of stale answers.
                return;
            }
            // clone: the departed set outlives the tree borrow it is read from
            let departed = partition.tree.iter().map(|entry| entry.entity_id.clone()).filter(|id| !incoming.contains(id)).collect();
            partition.tree = tree;
            partition.generation = snapshot_generation;
            departed
        };

        for entity_id in &incoming {
            let mut owned = self.by_entity.entry(entity_id.clone()).or_default(); // clone: one ownership record per entity in the snapshot
            if !owned.value().contains(&key) {
                owned.value_mut().push(key.clone()); // clone: one record per partition the entity appears in
            }
        }
        for entity_id in departed {
            if let Some(mut owned) = self.by_entity.get_mut(&entity_id) {
                owned.value_mut().retain(|held| held != &key);
            }
        }
    }

    /// Drop an entity from one partition, and forget that it was there.
    fn remove_from_key(&self, entity_id: &Urn, key: &SpatialKey) {
        self.remove_from_bucket(entity_id, key);
        if let Some(mut owned) = self.by_entity.get_mut(entity_id) {
            owned.value_mut().retain(|held| held != key);
        }
    }

    /// Drop an entity's entries from one partition's tree, advancing that
    /// partition's generation when anything was actually removed.
    fn remove_from_bucket(&self, entity_id: &Urn, key: &SpatialKey) {
        let Some(bucket) = self.buckets.get(key) else {
            return;
        };
        let mut partition = bucket.value().write();
        // clone: `RTree::remove` takes the entry by reference and the borrow of
        // the tree it came from cannot outlive the removal
        let to_remove: Vec<SpatialEntry> = partition.tree.iter().filter(|entry| entry.entity_id == *entity_id).cloned().collect();
        if to_remove.is_empty() {
            return;
        }
        for entry in to_remove {
            partition.tree.remove(&entry);
        }
        partition.generation = self.next_generation();
    }
}

/// Which entries of a partition a relation has to be evaluated against.
///
/// The R-tree can only rule out entries whose bounding box misses the
/// reference geometry's, which is sound for the containment and intersection
/// relations and unsound for the two that match *away* from the reference.
enum Candidates {
    /// Every entry in the partition.
    WholePartition,
    /// Only the entries whose envelope meets the reference envelope.
    EnvelopeIntersecting,
}

/// Candidate set a relation admits.
const fn candidates_for(georel: &GeoRel) -> Candidates {
    match georel {
        // No bounding-box cutoff is sound for `near`: `minDistance` matches
        // entries that are arbitrarily far from the reference envelope.
        GeoRel::Near { .. } |
        // `disjoint` matches precisely the entries the envelope test discards.
        GeoRel::Disjoint => Candidates::WholePartition,
        GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Equals | GeoRel::Overlaps => Candidates::EnvelopeIntersecting,
    }
}

/// Run the exact clause 4.10 predicate over whichever candidates the relation
/// admits.
fn evaluate(tree: &RTree<SpatialEntry>, georel: &GeoRel, reference: &geo::Geometry<f64>) -> HashSet<Urn> {
    let candidates: Vec<&SpatialEntry> = match candidates_for(georel) {
        Candidates::WholePartition => tree.iter().collect(),
        Candidates::EnvelopeIntersecting => tree.locate_in_envelope_intersecting(&geometry_envelope(reference)).collect(),
    };

    candidates
        .into_iter()
        .filter(|entry| matches_geo(georel, entry.geometry.as_ref(), reference))
        // clone: the returned set owns its identifiers, which outlive the tree borrow
        .map(|entry| entry.entity_id.clone())
        .collect()
}

/// The envelope of a reference geometry, or one covering the plane when it has
/// none — which turns the prefilter into a full scan rather than into an empty
/// answer.
fn geometry_envelope(geometry: &geo::Geometry<f64>) -> AABB<[f64; 2]> {
    match geometry.bounding_rect() {
        Some(rect) => AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]),
        None => AABB::from_corners([f64::NEG_INFINITY, f64::NEG_INFINITY], [f64::INFINITY, f64::INFINITY]),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{entry::SpatialEntry, index::SpatialEntityIndex},
        key::SpatialKey,
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use geo::{Coord, LineString, Point, Polygon};
    use iri_rs::{IriBuf, iri};
    use std::collections::HashSet;
    use urn_rs::Urn;

    fn key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/location")),
        )
    }

    fn urn(raw: &str) -> Urn {
        raw.parse().expect("a well-formed URN")
    }

    fn point(x: f64, y: f64) -> geo::Geometry<f64> {
        geo::Geometry::Point(Point::new(x, y))
    }

    fn square(cx: f64, cy: f64, half: f64) -> geo::Geometry<f64> {
        let coords = vec![
            Coord { x: cx - half, y: cy - half },
            Coord { x: cx + half, y: cy - half },
            Coord { x: cx + half, y: cy + half },
            Coord { x: cx - half, y: cy + half },
            Coord { x: cx - half, y: cy - half },
        ];
        geo::Geometry::Polygon(Polygon::new(LineString::from(coords), Vec::new()))
    }

    fn geometry_point(x: f64, y: f64) -> Geometry {
        Geometry::from(point(x, y))
    }

    fn geometry_square(cx: f64, cy: f64, half: f64) -> Geometry {
        Geometry::from(square(cx, cy, half))
    }

    #[test]
    fn an_entity_inside_the_reference_matches_within() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        index.insert(key(), id.clone(), geometry_point(0.0, 0.0));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 1.0)), Some(HashSet::from([id])));
    }

    #[test]
    fn an_entity_overlapping_the_reference_does_not_match_disjoint() {
        let index = SpatialEntityIndex::new();
        index.insert(key(), urn("urn:ngsi-ld:Vehicle:A"), geometry_square(0.0, 0.0, 2.0));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Disjoint, &square(0.0, 0.0, 1.0)), Some(HashSet::new()));
    }

    #[test]
    fn near_respects_the_maximum_distance() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        // 2 km east at the equator is ~0.01797 degrees of longitude.
        index.insert(key(), id.clone(), geometry_point(0.01797, 0.0));

        let tight = GeoRel::Near {
            max_distance: Some(1_000.0),
            min_distance: None,
        };
        let loose = GeoRel::Near {
            max_distance: Some(3_000.0),
            min_distance: None,
        };

        pretty_assertions::assert_eq!(index.query(&key(), &tight, &point(0.0, 0.0)), Some(HashSet::new()));
        pretty_assertions::assert_eq!(index.query(&key(), &loose, &point(0.0, 0.0)), Some(HashSet::from([id])));
    }

    #[test]
    fn near_matches_only_inside_the_band_the_two_distances_describe() {
        let index = SpatialEntityIndex::new();
        let inside = urn("urn:ngsi-ld:Vehicle:500m");
        // ~0.001 degrees is ~111 m at the equator.
        index.insert(key(), urn("urn:ngsi-ld:Vehicle:100m"), geometry_point(0.0009, 0.0));
        index.insert(key(), inside.clone(), geometry_point(0.0045, 0.0));
        index.insert(key(), urn("urn:ngsi-ld:Vehicle:2000m"), geometry_point(0.018, 0.0));

        let georel = GeoRel::Near {
            max_distance: Some(1_000.0),
            min_distance: Some(300.0),
        };
        pretty_assertions::assert_eq!(index.query(&key(), &georel, &point(0.0, 0.0)), Some(HashSet::from([inside])));
    }

    #[test]
    fn a_removed_entity_stops_matching() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        index.insert(key(), id.clone(), geometry_point(0.0, 0.0));
        index.remove(&id);
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(-10.0, -10.0, 20.0)), Some(HashSet::new()));
    }

    #[test]
    fn a_cold_partition_answers_nothing_rather_than_answering_empty() {
        let index = SpatialEntityIndex::new();
        assert!(index.query(&key(), &GeoRel::Intersects, &point(0.0, 0.0)).is_none());
    }

    #[test]
    fn a_bulk_loaded_partition_answers_the_same_as_one_built_by_inserts() {
        let inserted = SpatialEntityIndex::new();
        let loaded = SpatialEntityIndex::new();
        let mut entries = Vec::new();
        for step in 0..50 {
            let id = urn(&format!("urn:ngsi-ld:V:{step}"));
            let geometry = geometry_point(f64::from(step) * 0.001, 0.0);
            inserted.insert(key(), id.clone(), geometry.clone());
            if let Some(entry) = SpatialEntry::new(id, geometry) {
                entries.push(entry);
            }
        }
        loaded.bulk_load(key(), entries, loaded.current_generation());

        let reference = square(0.01, 0.0, 0.005);
        pretty_assertions::assert_eq!(
            inserted.query(&key(), &GeoRel::Intersects, &reference).unwrap_or_default(),
            loaded.query(&key(), &GeoRel::Intersects, &reference).unwrap_or_default()
        );
    }

    #[test]
    fn a_stale_rebuild_snapshot_does_not_clobber_a_newer_insert() {
        // The rebuild captures its snapshot generation, then a live insert lands
        // before the (empty) bulk-load is applied. The bulk-load must refuse to
        // overwrite the newer state, or the insert is lost.
        let index = SpatialEntityIndex::new();
        let snapshot = index.current_generation();
        let id = urn("urn:ngsi-ld:Vehicle:new");
        index.insert(key(), id.clone(), geometry_point(0.0, 0.0));

        index.bulk_load(key(), Vec::new(), snapshot);

        pretty_assertions::assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)).unwrap_or_default(),
            HashSet::from([id])
        );
    }

    #[test]
    fn a_rebuild_that_supersedes_the_snapshot_replaces_the_partition() {
        // When no newer write intervenes, the bulk-load applies normally: the
        // snapshot is at or ahead of the partition's generation.
        let index = SpatialEntityIndex::new();
        index.insert(key(), urn("urn:ngsi-ld:Vehicle:old"), geometry_point(0.0, 0.0));
        let snapshot = index.current_generation();
        let fresh = urn("urn:ngsi-ld:Vehicle:fresh");
        let entry = SpatialEntry::new(fresh.clone(), geometry_point(5.0, 5.0)).expect("an entry");

        index.bulk_load(key(), vec![entry], snapshot);

        assert!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)).unwrap_or_default().is_empty());
        pretty_assertions::assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(5.0, 5.0, 0.5)).unwrap_or_default(),
            HashSet::from([fresh])
        );
    }

    #[test]
    fn a_rebuild_prunes_departed_entities_from_the_ownership_record() {
        // An entity present before the rebuild but absent from its snapshot must
        // be forgotten in `by_entity`, or its stale partition record leaks and a
        // later removal cannot find it.
        let index = SpatialEntityIndex::new();
        let gone = urn("urn:ngsi-ld:Vehicle:gone");
        index.insert(key(), gone.clone(), geometry_point(0.0, 0.0));
        let snapshot = index.current_generation();
        let kept = urn("urn:ngsi-ld:Vehicle:kept");
        let entry = SpatialEntry::new(kept.clone(), geometry_point(5.0, 5.0)).expect("an entry");

        index.bulk_load(key(), vec![entry], snapshot);

        assert!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)).unwrap_or_default().is_empty());
        // Removing the departed entity is a no-op that leaves the kept one alone.
        index.remove(&gone);
        pretty_assertions::assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(5.0, 5.0, 0.5)).unwrap_or_default(),
            HashSet::from([kept])
        );
    }

    #[test]
    fn a_polygon_entity_contains_a_point_reference() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Building:Hall");
        index.insert(key(), id.clone(), geometry_square(0.0, 0.0, 1.0));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Contains, &point(0.5, 0.5)), Some(HashSet::from([id])));
    }

    #[test]
    fn equals_holds_only_for_the_same_geometry() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        index.insert(key(), id.clone(), geometry_square(0.0, 0.0, 1.0));

        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Equals, &square(0.0, 0.0, 1.0)), Some(HashSet::from([id])));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Equals, &square(0.0, 0.0, 2.0)), Some(HashSet::new()));
    }

    #[test]
    fn overlaps_matches_a_partial_intersection() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        index.insert(key(), id.clone(), geometry_square(0.0, 0.0, 1.0));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Overlaps, &square(0.5, 0.5, 1.0)), Some(HashSet::from([id])));
    }

    #[test]
    fn inserting_an_entity_twice_leaves_only_its_latest_geometry() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        index.insert(key(), id.clone(), geometry_point(0.0, 0.0));
        index.insert(key(), id.clone(), geometry_point(10.0, 10.0));

        assert!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)).unwrap_or_default().is_empty());
        pretty_assertions::assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(10.0, 10.0, 0.5)).unwrap_or_default(),
            HashSet::from([id])
        );
    }
}
