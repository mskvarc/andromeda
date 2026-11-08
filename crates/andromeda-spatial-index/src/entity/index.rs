//! The entity half of the spatial index: which entities a `georel` matches.

use crate::{
    entity::entry::SpatialEntry,
    key::SpatialKey,
    partitioned_rtree::{PartitionedRTree, WriteTracking},
};
use andromeda_ngsi_ld_geometry::{
    form::GeometryForm,
    geo_rel::GeoRel,
    great_circle::great_circle_bounds,
    near_envelope::NearEnvelope,
    planar::PlanarGeometry,
    relation::matches,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_tenancy::tenant_ref::TenantRef;
use dashmap::DashSet;
use rstar::{AABB, RTree};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// The set of partitions one rebuild covers: every entity type of one tenant
/// under one `GeoProperty`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SeedScope {
    /// The tenant the partitions belong to.
    tenant: TenantRef,
    /// The `GeoProperty` the partitions index.
    geoproperty: AttributeName,
}

/// Thread-safe R-tree cache of entity geometries.
///
/// A thin reading over the shared [`PartitionedRTree`]: the storage,
/// back-index, and rebuild machinery all live there, and this type adds the two
/// things that are entity-specific (evaluating a caller-supplied `georel`
/// against a partition, and knowing which partitions are complete enough to
/// answer one).
///
/// A partition answers only once a [`SpatialEntityIndex::rebuild`] has seeded
/// its `(tenant, geoproperty)` scope from the store. Live mutations keep a
/// seeded scope current, but live mutations alone never make a scope complete:
/// an entity stored before the broker started, or written by another broker
/// sharing the store, reaches the index only through a rebuild. Until then
/// [`SpatialEntityIndex::query`] declines, and the store evaluates the relation.
#[derive(Debug)]
pub struct SpatialEntityIndex {
    /// Entity geometries, partitioned by `(tenant, type, geoproperty)`.
    tree: PartitionedRTree<SpatialEntry>,
    /// The `(tenant, geoproperty)` scopes a rebuild has seeded.
    seeded: DashSet<SeedScope>,
}

// Manual because the partitioned tree must be told to record live mutations for
// a rebuild to respect, which no derived default could choose.
impl Default for SpatialEntityIndex {
    fn default() -> SpatialEntityIndex {
        SpatialEntityIndex::new()
    }
}

impl SpatialEntityIndex {
    /// An empty index with no seeded scope.
    #[must_use]
    pub fn new() -> SpatialEntityIndex {
        SpatialEntityIndex {
            tree: PartitionedRTree::new(WriteTracking::Recorded),
            seeded: DashSet::new(),
        }
    }

    /// The current clock value, to be captured by a rebuild worker *before* it
    /// reads its source snapshot so that any write applied during the read is
    /// stamped with a later generation and survives the rebuild.
    pub fn current_generation(&self) -> u64 {
        self.tree.current_generation()
    }

    /// The entities of one partition that may satisfy a `georel` filter, for
    /// the store to decide.
    ///
    /// `None` means the index declines to narrow, which is not the same answer
    /// as an empty set: the caller falls through to the store rather than
    /// reporting that nothing matched. It declines when the partition's scope
    /// has not been seeded, and for a `near` with only a `minDistance`, which
    /// no envelope can bound. A seeded scope with no partition for the key
    /// answers with the empty set, because its rebuild found no entity of that
    /// type conveying the `GeoProperty`.
    pub fn query(&self, key: &SpatialKey, georel: &GeoRel, reference: &GeometryForm) -> Option<HashSet<Identifier>> {
        let scope = SeedScope {
            tenant: key.tenant.clone(),
            geoproperty: key.geoproperty.clone(),
        };
        if !self.seeded.contains(&scope) {
            return None;
        }
        // Projected once: the candidate region and every candidate's relation
        // read the same projection.
        let reference = PlanarGeometry::project(reference);
        let candidates = candidates_for(georel, &reference)?;
        Some(
            self.tree
                .read_partition(key, |tree| evaluate(tree, georel, &reference, &candidates))
                .unwrap_or_default(),
        )
    }

    /// Replace every entry of one entity with the geometries it now conveys,
    /// one per partition it now belongs to.
    ///
    /// The entity leaves the partitions it no longer belongs to and swaps its
    /// entry in the ones it stays in under that partition's lock, so a reader
    /// never finds it missing from a partition it belongs to throughout. A
    /// geometry with no bounding rectangle cannot be indexed and is dropped.
    /// Each geometry is borrowed: the tree keeps only its projection.
    pub fn replace_entity<'g>(&self, entity_id: Arc<Identifier>, placements: impl IntoIterator<Item = (SpatialKey, &'g GeometryForm)>) {
        let entries = entries_of(&entity_id, placements);
        self.tree.replace(entity_id, entries);
    }

    /// Add geometries an entity may be about to convey, keeping every entry it
    /// already has.
    ///
    /// The entity stays reachable under its current geometries and becomes
    /// reachable under the new ones, so a query that runs while the change is
    /// pending sees it wherever the change may leave it. A later
    /// [`SpatialEntityIndex::replace_entity`] or [`SpatialEntityIndex::remove`]
    /// settles it.
    pub fn anticipate_entity<'g>(&self, entity_id: Arc<Identifier>, placements: impl IntoIterator<Item = (SpatialKey, &'g GeometryForm)>) {
        let entries = entries_of(&entity_id, placements);
        self.tree.augment(entity_id, entries);
    }

    /// Remove every entry belonging to an entity, across every partition.
    pub fn remove(&self, entity_id: &Identifier) {
        self.tree.remove(entity_id);
    }

    /// Rebuild every partition of one `(tenant, geoproperty)` scope from a
    /// snapshot of the store, and mark the scope seeded.
    ///
    /// `entries` holds the snapshot's entries by partition; a partition of the
    /// scope it does not mention is emptied. A live mutation applied after
    /// `snapshot_generation` was captured is kept over the snapshot. See
    /// [`PartitionedRTree::rebuild`].
    pub fn rebuild(&self, tenant: &TenantRef, geoproperty: &AttributeName, entries: HashMap<SpatialKey, Vec<SpatialEntry>>, snapshot_generation: u64) {
        self.tree
            .rebuild(|key| &key.tenant == tenant && &key.geoproperty == geoproperty, entries, snapshot_generation);
        self.seeded.insert(SeedScope {
            tenant: tenant.clone(),           // clone: the seeded set owns its tenant
            geoproperty: geoproperty.clone(), // clone: the seeded set owns its geoproperty
        });
    }
}

/// The tree entries of one entity's placements, grouped by partition.
fn entries_of<'g>(entity_id: &Arc<Identifier>, placements: impl IntoIterator<Item = (SpatialKey, &'g GeometryForm)>) -> Vec<(SpatialKey, Vec<SpatialEntry>)> {
    placements
        .into_iter()
        .filter_map(|(key, geometry)| SpatialEntry::new(Arc::clone(entity_id), geometry).map(|entry| (key, vec![entry])))
        .collect()
}

/// Which entries of a partition a relation has to be evaluated against.
///
/// The R-tree can only rule out entries whose box misses a region the relation
/// can reach, which is sound for the containment and intersection relations
/// and for `near` up to a `maxDistance`, and unsound for the ones that match
/// *away* from the reference.
enum Candidates {
    /// Every entry in the partition.
    WholePartition,
    /// Only the entries whose envelope meets the region.
    Envelope(NearEnvelope),
}

/// The candidate set a relation admits, or `None` when the index should not
/// narrow at all.
///
/// A reference with no coordinates has no region, so every entry is a
/// candidate.
fn candidates_for(georel: &GeoRel, reference: &PlanarGeometry) -> Option<Candidates> {
    match georel {
        // `disjoint` matches precisely the entries the envelope test discards.
        GeoRel::Disjoint => Some(Candidates::WholePartition),
        // A `minDistance` alone matches entries arbitrarily far from the
        // reference, so no envelope bounds it, and every entry of the partition
        // would be a candidate the store has to bind and re-check. Declining
        // lets the store evaluate the relation over its own rows instead.
        GeoRel::Near { max_distance: None, .. } => None,
        // Clause 4.10 `near` with a `maxDistance` reaches only what lies within
        // that distance of the reference, measured along great-circle edges on
        // the spheroid; the envelope covers all of it.
        GeoRel::Near {
            max_distance: Some(max_distance),
            ..
        } => Some(great_circle_bounds(reference).map_or(Candidates::WholePartition, |rect| {
            Candidates::Envelope(NearEnvelope::around(rect, *max_distance))
        })),
        GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Equals | GeoRel::Overlaps => Some(
            reference
                .bounding_rect()
                .map_or(Candidates::WholePartition, |rect| Candidates::Envelope(NearEnvelope::Single(rect))),
        ),
    }
}

/// Whether the in-memory relation may decide membership, or only the envelope
/// test may.
///
/// The index is an optimisation, so it may hand the store more candidates than
/// match but never fewer. That rules the in-memory predicate out wherever its
/// reading is not the store's to the last bit.
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
        // of clause 4.10 `equals` is mutual containment, which excludes shared
        // boundaries and is therefore strictly stronger than the `ST_Equals`
        // set equality the store applies. Refining with it would drop entities
        // the store counts as equal.
        GeoRel::Equals |
        // The in-memory `near` measures what `geography` measures, but decides
        // containment on the plane and runs its own geodesic arithmetic, so a
        // pair at the threshold can fall either side of it. The store
        // re-applies the relation to every candidate anyway, and refining
        // would make an answer depend on two implementations agreeing exactly;
        // a centroid reading of `near` would drop every building whose middle
        // is out of range but whose nearest wall is not.
        GeoRel::Near { .. } => Refinement::EnvelopeOnly,
        GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Disjoint | GeoRel::Overlaps => Refinement::Exact,
    }
}

/// Run the clause 4.10 predicate over whichever candidates the relation admits.
///
/// An entry that straddles the antimeridian can meet both halves of a split
/// envelope; the set reports it once.
fn evaluate(tree: &RTree<SpatialEntry>, georel: &GeoRel, reference: &PlanarGeometry, candidates: &Candidates) -> HashSet<Identifier> {
    let entries: Vec<&SpatialEntry> = match candidates {
        Candidates::WholePartition => tree.iter().collect(),
        Candidates::Envelope(envelope) => envelope
            .rects()
            .iter()
            .flat_map(|rect| tree.locate_in_envelope_intersecting(AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])))
            .collect(),
    };

    entries
        .into_iter()
        .filter(|entry| match refinement_for(georel) {
            Refinement::Exact => matches(georel, &entry.planar, reference),
            Refinement::EnvelopeOnly => true,
        })
        // clone: the returned set owns its identifiers, which outlive the tree borrow
        .map(|entry| Identifier::clone(&entry.entity_id))
        .collect()
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
        linear_ring::LinearRing,
        position::Position,
        positive_distance::PositiveDistance,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};
    use pretty_assertions::assert_eq;
    use std::{
        collections::{HashMap, HashSet},
        str::FromStr,
        sync::Arc,
    };

    fn location() -> AttributeName {
        "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name")
    }

    fn key() -> SpatialKey {
        SpatialKey::new(TenantRef::UnnamedDefault, IriBuf::from(iri!("https://example.org/Vehicle")), location())
    }

    fn uri(raw: &str) -> Arc<Identifier> {
        Arc::new(raw.parse().expect("a well-formed URI"))
    }

    /// The set a query answers with when it finds exactly these entities.
    fn identifiers<const N: usize>(ids: [Arc<Identifier>; N]) -> HashSet<Identifier> {
        ids.into_iter().map(Arc::unwrap_or_clone).collect()
    }

    fn point(x: f64, y: f64) -> GeometryForm {
        GeometryForm::Point(Position::try_new(x, y, None).expect("a test position lies on the CRS84 axes"))
    }

    fn square(cx: f64, cy: f64, half: f64) -> GeometryForm {
        GeometryForm::Polygon(PolygonRings::new(vec![
            LinearRing::try_new(vec![
                Position::try_new(cx - half, cy - half, None).expect("a test position lies on the CRS84 axes"),
                Position::try_new(cx + half, cy - half, None).expect("a test position lies on the CRS84 axes"),
                Position::try_new(cx + half, cy + half, None).expect("a test position lies on the CRS84 axes"),
                Position::try_new(cx - half, cy + half, None).expect("a test position lies on the CRS84 axes"),
                Position::try_new(cx - half, cy - half, None).expect("a test position lies on the CRS84 axes"),
            ])
            .expect("a test square is a closed ring"),
        ]))
    }

    /// An index whose `location` scope a rebuild over an empty store seeded.
    fn seeded() -> SpatialEntityIndex {
        let index = SpatialEntityIndex::new();
        index.rebuild(&TenantRef::UnnamedDefault, &location(), HashMap::new(), index.current_generation());
        index
    }

    #[test]
    fn an_unseeded_scope_declines_to_answer_even_with_live_entries() {
        let index = SpatialEntityIndex::new();
        index.replace_entity(uri("urn:ngsi-ld:Vehicle:A"), [(key(), &point(0.0, 0.0))]);
        assert!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 1.0)).is_none());
    }

    #[test]
    fn a_seeded_scope_answers_empty_for_a_type_it_holds_nothing_of() {
        assert_eq!(seeded().query(&key(), &GeoRel::Intersects, &point(0.0, 0.0)), Some(HashSet::new()));
    }

    #[test]
    fn a_seed_covers_only_its_own_geoproperty() {
        let index = seeded();
        let other = SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            "https://example.org/area".parse().expect("valid attribute name"),
        );
        assert!(index.query(&other, &GeoRel::Intersects, &point(0.0, 0.0)).is_none());
    }

    #[test]
    fn an_entity_identified_by_an_http_iri_is_indexed() {
        let index = seeded();
        let id = uri("https://example.org/vehicles/1");
        index.replace_entity(id.clone(), [(key(), &point(0.0, 0.0))]);
        assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 1.0)), Some(identifiers([id])));
    }

    #[test]
    fn an_entity_inside_the_reference_matches_within() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Vehicle:A");
        index.replace_entity(id.clone(), [(key(), &point(0.0, 0.0))]);
        assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 1.0)), Some(identifiers([id])));
    }

    #[test]
    fn an_entity_overlapping_the_reference_does_not_match_disjoint() {
        let index = seeded();
        index.replace_entity(uri("urn:ngsi-ld:Vehicle:A"), [(key(), &square(0.0, 0.0, 2.0))]);
        assert_eq!(index.query(&key(), &GeoRel::Disjoint, &square(0.0, 0.0, 1.0)), Some(HashSet::new()));
    }

    #[test]
    fn near_respects_the_maximum_distance() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Vehicle:A");
        // 2 km east at the equator is ~0.01797 degrees of longitude.
        index.replace_entity(id.clone(), [(key(), &point(0.01797, 0.0))]);

        let tight = GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
            min_distance: None,
        };
        let loose = GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str("3000").expect("positive distance")),
            min_distance: None,
        };

        assert_eq!(index.query(&key(), &tight, &point(0.0, 0.0)), Some(HashSet::new()));
        assert_eq!(index.query(&key(), &loose, &point(0.0, 0.0)), Some(identifiers([id])));
    }

    fn near(max_distance: Option<&str>, min_distance: Option<&str>) -> GeoRel {
        GeoRel::Near {
            max_distance: max_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: min_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
        }
    }

    /// A polygon whose rings are the given longitude/latitude boxes.
    fn boxes(rings: &[(f64, f64, f64, f64)]) -> Vec<PolygonRings> {
        rings
            .iter()
            .map(|&(west, south, east, north)| {
                PolygonRings::new(vec![
                    LinearRing::try_new(
                        [(west, south), (east, south), (east, north), (west, north), (west, south)]
                            .into_iter()
                            .map(|(x, y)| Position::try_new(x, y, None).expect("a test position lies on the CRS84 axes"))
                            .collect(),
                    )
                    .expect("a test box is a closed ring"),
                ])
            })
            .collect()
    }

    /// Bremen, the reference point the minimum-distance fixtures sit around.
    fn bremen() -> GeometryForm {
        point(8.8072, 53.0758)
    }

    /// The `minDistance` half of a band is the store's to decide: the index
    /// hands it every entry within the `maxDistance` envelope.
    #[test]
    fn near_with_both_bounds_returns_the_max_distance_envelope_candidates() {
        let index = seeded();
        let close = uri("urn:ngsi-ld:Vehicle:100m");
        let inside = uri("urn:ngsi-ld:Vehicle:500m");
        // ~0.001 degrees is ~111 m at the equator.
        index.replace_entity(close.clone(), [(key(), &point(0.0009, 0.0))]);
        index.replace_entity(inside.clone(), [(key(), &point(0.0045, 0.0))]);
        index.replace_entity(uri("urn:ngsi-ld:Vehicle:2000m"), [(key(), &point(0.018, 0.0))]);

        assert_eq!(
            index.query(&key(), &near(Some("1000"), Some("300")), &point(0.0, 0.0)),
            Some(identifiers([close, inside]))
        );
    }

    /// Clause 4.10 `near` measures the minimum distance: a building whose
    /// nearest wall is 100.06 m away is within 150 m, though its centroid is
    /// 170.10 m away.
    #[test]
    fn near_finds_a_polygon_whose_nearest_wall_is_in_range() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Building:east");
        let east_square = GeometryForm::Polygon(boxes(&[(8.808_693, 53.075_171, 8.810_783, 53.076_429)]).remove(0));
        index.replace_entity(id.clone(), [(key(), &east_square)]);
        assert_eq!(index.query(&key(), &near(Some("150"), None), &bremen()), Some(identifiers([id])));
    }

    /// Whether an envelope candidate is within the distance is the store's
    /// decision, so a point inside the box but outside the circle is handed on.
    #[test]
    fn near_hands_the_store_every_envelope_candidate() {
        let index = seeded();
        // About 184 m from Bremen, in the corner of the 150 m box.
        let id = uri("urn:ngsi-ld:Building:corner");
        index.replace_entity(id.clone(), [(key(), &point(8.8091, 53.0770))]);
        assert_eq!(index.query(&key(), &near(Some("150"), None), &bremen()), Some(identifiers([id])));
    }

    #[test]
    fn a_min_distance_only_near_declines_to_narrow() {
        let index = seeded();
        index.replace_entity(uri("urn:ngsi-ld:Building:any"), [(key(), &bremen())]);
        assert_eq!(index.query(&key(), &near(None, Some("150")), &bremen()), None);
    }

    #[test]
    fn near_across_the_antimeridian_finds_entries_on_both_sides() {
        let index = seeded();
        let east = uri("urn:ngsi-ld:Building:east-of-180");
        let west = uri("urn:ngsi-ld:Building:west-of-180");
        index.replace_entity(east.clone(), [(key(), &point(179.9995, -17.0))]);
        index.replace_entity(west.clone(), [(key(), &point(-179.9995, -17.0))]);
        assert_eq!(
            index.query(&key(), &near(Some("300"), None), &point(179.999, -17.0)),
            Some(identifiers([east, west]))
        );
    }

    #[test]
    fn a_seam_straddling_entry_is_reported_once() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Building:straddling");
        let cut = GeometryForm::MultiPolygon(boxes(&[(179.9990, -17.0005, 180.0, -16.9995), (-180.0, -17.0005, -179.9990, -16.9995)]));
        index.replace_entity(id.clone(), [(key(), &cut)]);
        assert_eq!(index.query(&key(), &near(Some("300"), None), &point(179.999, -17.0)), Some(identifiers([id])));
    }

    #[test]
    fn a_removed_entity_stops_matching() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Vehicle:A");
        index.replace_entity(id.clone(), [(key(), &point(0.0, 0.0))]);
        index.remove(&id);
        assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(-10.0, -10.0, 20.0)), Some(HashSet::new()));
    }

    #[test]
    fn a_rebuilt_scope_answers_the_same_as_one_built_by_replacements() {
        let replaced = seeded();
        let rebuilt = SpatialEntityIndex::new();
        let mut entries = Vec::new();
        for step in 0..50 {
            let id = uri(&format!("urn:ngsi-ld:V:{step}"));
            let geometry = point(f64::from(step) * 0.001, 0.0);
            entries.extend(SpatialEntry::new(id.clone(), &geometry));
            replaced.replace_entity(id, [(key(), &geometry)]);
        }
        rebuilt.rebuild(
            &TenantRef::UnnamedDefault,
            &location(),
            HashMap::from([(key(), entries)]),
            rebuilt.current_generation(),
        );

        let reference = square(0.01, 0.0, 0.005);
        assert_eq!(
            replaced.query(&key(), &GeoRel::Intersects, &reference),
            rebuilt.query(&key(), &GeoRel::Intersects, &reference)
        );
    }

    #[test]
    fn a_stale_rebuild_snapshot_does_not_clobber_a_newer_replacement() {
        // The rebuild captures its snapshot generation, then a live write lands
        // before the (empty) snapshot is applied. The rebuild must keep the
        // newer state, or the write is lost.
        let index = SpatialEntityIndex::new();
        let snapshot = index.current_generation();
        let id = uri("urn:ngsi-ld:Vehicle:new");
        index.replace_entity(id.clone(), [(key(), &point(0.0, 0.0))]);

        index.rebuild(&TenantRef::UnnamedDefault, &location(), HashMap::new(), snapshot);

        assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)), Some(identifiers([id])));
    }

    #[test]
    fn an_anticipated_geometry_widens_the_candidates_until_the_change_settles() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Vehicle:A");
        index.replace_entity(id.clone(), [(key(), &point(0.0, 0.0))]);
        index.anticipate_entity(id.clone(), [(key(), &point(10.0, 10.0))]);

        assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)),
            Some(identifiers([id.clone()]))
        );
        assert_eq!(
            index.query(&key(), &GeoRel::Intersects, &square(10.0, 10.0, 0.5)),
            Some(identifiers([id.clone()]))
        );

        index.replace_entity(id.clone(), [(key(), &point(10.0, 10.0))]);
        assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)), Some(HashSet::new()));
        assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(10.0, 10.0, 0.5)), Some(identifiers([id])));
    }

    #[test]
    fn a_polygon_entity_contains_a_point_reference() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Building:Hall");
        index.replace_entity(id.clone(), [(key(), &square(0.0, 0.0, 1.0))]);
        assert_eq!(index.query(&key(), &GeoRel::Contains, &point(0.5, 0.5)), Some(identifiers([id])));
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
        let index = seeded();
        let id = uri("urn:ngsi-ld:Region:A");
        index.replace_entity(id.clone(), [(key(), &square(0.0, 0.0, 1.0))]);

        assert_eq!(index.query(&key(), &GeoRel::Equals, &square(0.0, 0.0, 1.0)), Some(identifiers([id.clone()])));
        assert_eq!(index.query(&key(), &GeoRel::Equals, &square(0.0, 0.0, 2.0)), Some(identifiers([id])));
        assert_eq!(index.query(&key(), &GeoRel::Equals, &square(50.0, 50.0, 1.0)), Some(HashSet::new()));
    }

    /// Clause 4.10: an entity matches when any instance of the target
    /// `GeoProperty` meets the restriction, so a partition may hold one entry
    /// per instance and reports the entity once however many match.
    #[test]
    fn every_placement_of_an_entity_in_one_partition_is_indexed_and_reported_once() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Region:A");
        index.replace_entity(id.clone(), [(key(), &square(0.0, 0.0, 1.0)), (key(), &square(40.0, 40.0, 1.0))]);

        assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 5.0)), Some(identifiers([id.clone()])));
        assert_eq!(index.query(&key(), &GeoRel::Within, &square(40.0, 40.0, 5.0)), Some(identifiers([id.clone()])));
        assert_eq!(index.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 80.0)), Some(identifiers([id])));
    }

    #[test]
    fn replacing_an_entity_drops_the_placements_it_no_longer_has() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Region:A");
        index.replace_entity(id.clone(), [(key(), &square(0.0, 0.0, 1.0)), (key(), &square(40.0, 40.0, 1.0))]);
        index.replace_entity(id.clone(), [(key(), &square(0.0, 0.0, 1.0))]);

        assert_eq!(index.query(&key(), &GeoRel::Within, &square(40.0, 40.0, 5.0)), Some(HashSet::new()));
        assert_eq!(index.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 5.0)), Some(identifiers([id])));
    }

    #[test]
    fn overlaps_matches_a_partial_intersection() {
        let index = seeded();
        let id = uri("urn:ngsi-ld:Region:A");
        index.replace_entity(id.clone(), [(key(), &square(0.0, 0.0, 1.0))]);
        assert_eq!(index.query(&key(), &GeoRel::Overlaps, &square(0.5, 0.5, 1.0)), Some(identifiers([id])));
    }
}
