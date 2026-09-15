//! The entity half of the spatial index: which entities a `georel` matches.

use crate::{entity::entry::SpatialEntry, key::SpatialKey, partitioned_rtree::PartitionedRTree};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, relation::matches};
use rstar::{AABB, RTree};
use std::collections::HashSet;
use urn_rs::Urn;

/// Thread-safe R-tree cache of entity geometries.
///
/// A thin reading over the shared [`PartitionedRTree`]: the storage,
/// back-index, and rebuild machinery all live there, and this type adds only
/// the one thing that is entity-specific — evaluating a caller-supplied
/// `georel` against a partition.
#[derive(Debug, Default)]
pub struct SpatialEntityIndex {
    /// Entity geometries, partitioned by `(tenant, type, geoproperty)`.
    tree: PartitionedRTree<SpatialEntry>,
}

impl SpatialEntityIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> SpatialEntityIndex {
        SpatialEntityIndex::default()
    }

    /// The current clock value, to be captured by a rebuild worker *before* it
    /// reads its source snapshot so that any write landing during the read is
    /// stamped with a later generation and cannot be clobbered by the rebuild.
    pub fn current_generation(&self) -> u64 {
        self.tree.current_generation()
    }

    /// Evaluate a `georel` filter against one partition.
    ///
    /// `None` means the partition is cold, which is not the same answer as an
    /// empty set: the caller falls through to the store rather than reporting
    /// that nothing matched.
    pub fn query(&self, key: &SpatialKey, georel: &GeoRel, reference: &Geometry) -> Option<HashSet<Urn>> {
        self.tree.read_partition(key, |tree| evaluate(tree, georel, reference))
    }

    /// Replace one entity's geometries under one partition.
    ///
    /// ETSI GS CIM 009 v1.9.1 clause 4.10 counts an entity as matching when any
    /// one instance of the target `GeoProperty` meets the restriction, so every
    /// `datasetId` instance is indexed and the partition may hold the entity
    /// several times. [`SpatialEntityIndex::query`] answers with a set, so an
    /// entity two of whose instances match is still reported once.
    ///
    /// A geometry with no bounding rectangle cannot be indexed and is dropped;
    /// an entity left with no entry at all is simply absent from the partition.
    pub fn insert_all(&self, key: SpatialKey, entity_id: Urn, geometries: impl IntoIterator<Item = Geometry>) {
        let entries: Vec<SpatialEntry> = geometries
            .into_iter()
            // clone: each entry owns the identifier the back-index is keyed by
            .filter_map(|geometry| SpatialEntry::new(entity_id.clone(), geometry))
            .collect();
        self.tree.insert_all(key, entity_id, entries);
    }

    /// Remove every entry belonging to an entity, across every partition.
    pub fn remove(&self, entity_id: &Urn) {
        self.tree.remove(entity_id);
    }

    /// Replace one partition's contents wholesale, unless a newer write got
    /// there first. See [`PartitionedRTree::bulk_load`].
    pub fn bulk_load(&self, key: SpatialKey, entries: Vec<SpatialEntry>, snapshot_generation: u64) {
        self.tree.bulk_load(key, entries, snapshot_generation);
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

/// Whether the in-memory relation may decide membership, or only the envelope
/// test may.
///
/// The index is an optimisation, so it may hand the store more candidates than
/// match but never fewer. That rules the in-memory predicate out wherever its
/// reading is strictly stronger than the store's.
enum Refinement {
    /// The in-memory relation is the predicate the store applies.
    Exact,
    /// Only the envelope test may be trusted; the store decides membership.
    EnvelopeOnly,
}

/// Whether a relation's in-memory reading agrees with the store's.
const fn refinement_for(georel: &GeoRel) -> Refinement {
    match georel {
        // `geo` has no topological equality predicate, so the in-memory reading
        // of clause 4.10 `equals` is mutual containment — which excludes shared
        // boundaries and is therefore strictly stronger than the `ST_Equals`
        // set equality the store applies. Refining with it would drop entities
        // the store counts as equal.
        GeoRel::Equals => Refinement::EnvelopeOnly,
        GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Disjoint | GeoRel::Overlaps | GeoRel::Near { .. } => Refinement::Exact,
    }
}

/// Run the clause 4.10 predicate over whichever candidates the relation admits.
fn evaluate(tree: &RTree<SpatialEntry>, georel: &GeoRel, reference: &Geometry) -> HashSet<Urn> {
    let candidates: Vec<&SpatialEntry> = match candidates_for(georel) {
        Candidates::WholePartition => tree.iter().collect(),
        Candidates::EnvelopeIntersecting => tree.locate_in_envelope_intersecting(geometry_envelope(reference)).collect(),
    };

    candidates
        .into_iter()
        .filter(|entry| match refinement_for(georel) {
            Refinement::Exact => matches(georel, &entry.geometry, reference),
            Refinement::EnvelopeOnly => true,
        })
        // clone: the returned set owns its identifiers, which outlive the tree borrow
        .map(|entry| entry.entity_id.clone())
        .collect()
}

/// The envelope of a reference geometry, or one covering the plane when it has
/// none — which turns the prefilter into a full scan rather than into an empty
/// answer.
fn geometry_envelope(geometry: &Geometry) -> AABB<[f64; 2]> {
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
    use andromeda_ngsi_ld_geometry::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        geometry::Geometry,
        position::Position,
        positive_distance::PositiveDistance,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};
    use std::{collections::HashSet, str::FromStr};
    use urn_rs::Urn;

    fn key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        )
    }

    fn urn(raw: &str) -> Urn {
        raw.parse().expect("a well-formed URN")
    }

    fn point(x: f64, y: f64) -> Geometry {
        Geometry::new(GeometryForm::Point(Position::new(x, y, None)))
    }

    fn square(cx: f64, cy: f64, half: f64) -> Geometry {
        Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![vec![
            Position::new(cx - half, cy - half, None),
            Position::new(cx + half, cy - half, None),
            Position::new(cx + half, cy + half, None),
            Position::new(cx - half, cy + half, None),
            Position::new(cx - half, cy - half, None),
        ]])))
    }

    #[test]
    fn an_entity_inside_the_reference_matches_within() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        index.insert_all(key(), id.clone(), [point(0.0, 0.0)]);
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 1.0)), Some(HashSet::from([id])));
    }

    #[test]
    fn an_entity_overlapping_the_reference_does_not_match_disjoint() {
        let index = SpatialEntityIndex::new();
        index.insert_all(key(), urn("urn:ngsi-ld:Vehicle:A"), [square(0.0, 0.0, 2.0)]);
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Disjoint, &square(0.0, 0.0, 1.0)), Some(HashSet::new()));
    }

    #[test]
    fn near_respects_the_maximum_distance() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        // 2 km east at the equator is ~0.01797 degrees of longitude.
        index.insert_all(key(), id.clone(), [point(0.01797, 0.0)]);

        let tight = GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
            min_distance: None,
        };
        let loose = GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("3000").expect("positive distance")),
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
        index.insert_all(key(), urn("urn:ngsi-ld:Vehicle:100m"), [point(0.0009, 0.0)]);
        index.insert_all(key(), inside.clone(), [point(0.0045, 0.0)]);
        index.insert_all(key(), urn("urn:ngsi-ld:Vehicle:2000m"), [point(0.018, 0.0)]);

        let georel = GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
            min_distance: Some(PositiveDistance::from_str("300").expect("positive distance")),
        };
        pretty_assertions::assert_eq!(index.query(&key(), &georel, &point(0.0, 0.0)), Some(HashSet::from([inside])));
    }

    #[test]
    fn a_removed_entity_stops_matching() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        index.insert_all(key(), id.clone(), [point(0.0, 0.0)]);
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
            let geometry = point(f64::from(step) * 0.001, 0.0);
            inserted.insert_all(key(), id.clone(), [geometry.clone()]);
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
        index.insert_all(key(), id.clone(), [point(0.0, 0.0)]);

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
        index.insert_all(key(), urn("urn:ngsi-ld:Vehicle:old"), [point(0.0, 0.0)]);
        let snapshot = index.current_generation();
        let fresh = urn("urn:ngsi-ld:Vehicle:fresh");
        let entry = SpatialEntry::new(fresh.clone(), point(5.0, 5.0)).expect("an entry");

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
        index.insert_all(key(), gone.clone(), [point(0.0, 0.0)]);
        let snapshot = index.current_generation();
        let kept = urn("urn:ngsi-ld:Vehicle:kept");
        let entry = SpatialEntry::new(kept.clone(), point(5.0, 5.0)).expect("an entry");

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
        index.insert_all(key(), id.clone(), [square(0.0, 0.0, 1.0)]);
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Contains, &point(0.5, 0.5)), Some(HashSet::from([id])));
    }

    /// `equals` is answered by the envelope test alone.
    ///
    /// `geo` offers no topological equality, and the mutual-containment reading
    /// standing in for it excludes shared boundaries, so it is strictly
    /// stronger than the `ST_Equals` the store applies. The index therefore
    /// keeps every envelope-intersecting entry and leaves the decision to the
    /// store, because handing back fewer candidates than match is the one thing
    /// an optimisation may not do.
    #[test]
    fn equals_admits_every_envelope_intersecting_candidate_and_leaves_the_decision_to_the_store() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        index.insert_all(key(), id.clone(), [square(0.0, 0.0, 1.0)]);

        // clone: each assertion owns the identifier it compares against
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Equals, &square(0.0, 0.0, 1.0)), Some(HashSet::from([id.clone()])));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Equals, &square(0.0, 0.0, 2.0)), Some(HashSet::from([id])));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Equals, &square(50.0, 50.0, 1.0)), Some(HashSet::new()));
    }

    /// Clause 4.10: an entity matches when any instance of the target
    /// `GeoProperty` meets the restriction, so a partition holds one entry per
    /// `datasetId` instance and reports the entity once however many match.
    #[test]
    fn every_instance_of_a_multi_dataset_geoproperty_is_indexed_and_reported_once() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        index.insert_all(key(), id.clone(), [square(0.0, 0.0, 1.0), square(40.0, 40.0, 1.0)]);

        // clone: each assertion owns the identifier it compares against
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 5.0)), Some(HashSet::from([id.clone()])));
        pretty_assertions::assert_eq!(
            index.query(&key(), &GeoRel::Within, &square(40.0, 40.0, 5.0)),
            Some(HashSet::from([id.clone()]))
        );
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 100.0)), Some(HashSet::from([id])));
    }

    /// Replacing an entity's entries drops the instances it no longer carries.
    #[test]
    fn replacing_an_entitys_instances_drops_the_ones_it_no_longer_carries() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        index.insert_all(key(), id.clone(), [square(0.0, 0.0, 1.0), square(40.0, 40.0, 1.0)]);
        index.insert_all(key(), id.clone(), [square(0.0, 0.0, 1.0)]);

        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Within, &square(40.0, 40.0, 5.0)), Some(HashSet::new()));
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 5.0)), Some(HashSet::from([id])));
    }

    #[test]
    fn overlaps_matches_a_partial_intersection() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        index.insert_all(key(), id.clone(), [square(0.0, 0.0, 1.0)]);
        pretty_assertions::assert_eq!(index.query(&key(), &GeoRel::Overlaps, &square(0.5, 0.5, 1.0)), Some(HashSet::from([id])));
    }

    #[test]
    fn inserting_an_entity_twice_leaves_only_its_latest_geometry() {
        let index = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        index.insert_all(key(), id.clone(), [point(0.0, 0.0)]);
        index.insert_all(key(), id.clone(), [point(10.0, 10.0)]);

        assert!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)).unwrap_or_default().is_empty());
        pretty_assertions::assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(10.0, 10.0, 0.5)).unwrap_or_default(),
            HashSet::from([id])
        );
    }
}
