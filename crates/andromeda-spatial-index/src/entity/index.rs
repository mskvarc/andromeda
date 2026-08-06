//! The R-tree of entity geometries.

use crate::{entity::entry::SpatialEntry, key::SpatialKey};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, relation::matches_geo};
use dashmap::DashMap;
use geo::algorithm::BoundingRect;
use parking_lot::RwLock;
use rstar::{AABB, RTree};
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

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
    buckets: DashMap<SpatialKey, Arc<RwLock<RTree<SpatialEntry>>>>,
    /// Which partitions each entity currently appears in.
    by_entity: DashMap<Urn, Vec<SpatialKey>>,
}

impl SpatialEntityIndex {
    /// An empty index.
    pub fn new() -> SpatialEntityIndex {
        SpatialEntityIndex::default()
    }

    /// Evaluate a `georel` filter against one partition.
    ///
    /// `None` means the partition is cold, which is not the same answer as an
    /// empty set: the caller falls through to the store rather than reporting
    /// that nothing matched.
    pub fn query(&self, key: &SpatialKey, georel: &GeoRel, reference: &geo::Geometry<f64>) -> Option<HashSet<Urn>> {
        let bucket = self.buckets.get(key)?;
        let tree = bucket.value().read();
        Some(evaluate(&tree, georel, reference))
    }

    /// Insert or overwrite the entry for one entity under one partition.
    pub fn insert(&self, key: SpatialKey, entity_id: Urn, geometry: Geometry) {
        let Some(entry) = SpatialEntry::new(entity_id.clone(), geometry) else {
            // clone: the entry owns its identifier and the caller keeps one
            return;
        };

        self.remove_from_key(&entity_id, &key);

        let bucket = self.buckets.entry(key.clone()).or_insert_with(|| Arc::new(RwLock::new(RTree::new()))); // clone: the bucket key and the ownership record are separate
        bucket.value().write().insert(entry);

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

    /// Replace one partition's contents wholesale.
    ///
    /// Bulk-loading yields a much better tree layout than the same entries
    /// inserted one at a time, and the swap happens under the existing lock so
    /// an in-flight reader sees either the old tree or the new one and never a
    /// partial state.
    pub fn bulk_load(&self, key: SpatialKey, entries: Vec<SpatialEntry>) {
        for entry in &entries {
            let mut owned = self.by_entity.entry(entry.entity_id.clone()).or_default(); // clone: the ownership record outlives the entry list
            if !owned.value().contains(&key) {
                owned.value_mut().push(key.clone()); // clone: one record per partition the entity appears in
            }
        }
        let tree = RTree::bulk_load(entries);
        let bucket = self.buckets.entry(key).or_insert_with(|| Arc::new(RwLock::new(RTree::new())));
        *bucket.value().write() = tree;
    }

    /// Drop an entity from one partition, and forget that it was there.
    fn remove_from_key(&self, entity_id: &Urn, key: &SpatialKey) {
        self.remove_from_bucket(entity_id, key);
        if let Some(mut owned) = self.by_entity.get_mut(entity_id) {
            owned.value_mut().retain(|held| held != key);
        }
    }

    /// Drop an entity's entries from one partition's tree.
    fn remove_from_bucket(&self, entity_id: &Urn, key: &SpatialKey) {
        let Some(bucket) = self.buckets.get(key) else {
            return;
        };
        let mut tree = bucket.value().write();
        // clone: `RTree::remove` takes the entry by reference and the borrow of
        // the tree it came from cannot outlive the removal
        let to_remove: Vec<SpatialEntry> = tree.iter().filter(|entry| entry.entity_id == *entity_id).cloned().collect();
        for entry in to_remove {
            tree.remove(&entry);
        }
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
        loaded.bulk_load(key(), entries);

        let reference = square(0.01, 0.0, 0.005);
        pretty_assertions::assert_eq!(
            inserted.query(&key(), &GeoRel::Intersects, &reference).unwrap_or_default(),
            loaded.query(&key(), &GeoRel::Intersects, &reference).unwrap_or_default()
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
