//! The subscription half of the spatial index: which subscriptions an entity
//! change is a candidate for.

use crate::{
    key::SpatialKey,
    partitioned_rtree::{Indexed, PartitionedRTree, WriteTracking},
    subscription::entry::SubEntry,
};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, great_circle::great_circle_bounds, planar::PlanarGeometry, relation::matches};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use rstar::{AABB, Envelope, RTree};

/// Thread-safe R-tree cache of subscription reference geometries.
///
/// The inverse of the entity index: one entity change is matched against
/// O(log n) candidate subscriptions instead of every geo-enabled subscription
/// the broker holds. A thin reading over the shared [`PartitionedRTree`] that
/// adds only the candidate evaluation, which is where the two indexes differ:
/// each entry carries its own `georel`, so the match is applied per entry
/// rather than against one caller-supplied relation.
#[derive(Debug)]
pub struct SpatialSubscriptionIndex {
    /// Subscription reference geometries, partitioned by
    /// `(tenant, type, geoproperty)`.
    tree: PartitionedRTree<SubEntry>,
}

// Manual because the partitioned tree must be told not to record live
// mutations, which no derived default could choose.
impl Default for SpatialSubscriptionIndex {
    fn default() -> SpatialSubscriptionIndex {
        SpatialSubscriptionIndex::new()
    }
}

impl SpatialSubscriptionIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> SpatialSubscriptionIndex {
        // The subscription cache mirrors the subscription store change by
        // change and is reloaded wholesale rather than rebuilt from a snapshot,
        // so nothing is recorded for a rebuild to respect.
        SpatialSubscriptionIndex {
            tree: PartitionedRTree::new(WriteTracking::Unrecorded),
        }
    }

    /// Every subscription in one partition whose geo filter the given entity
    /// geometry satisfies.
    ///
    /// Takes the entity's horizontal projection rather than its geometry, so a
    /// caller probing several partitions with one geometry projects it once and
    /// the index never copies the positions.
    /// Already exact: each candidate the tree admits is put through the clause
    /// 4.10 predicate before it is returned, so the caller needs no second
    /// pass.
    pub fn candidates(&self, key: &SpatialKey, entity_geometry: &PlanarGeometry) -> Vec<Identifier> {
        // Great-circle bounds, so a `near` subscription is found beside the
        // bulge of a long entity edge as well as beside its vertices.
        let Some(rect) = great_circle_bounds(entity_geometry) else {
            return Vec::new();
        };
        let envelope = AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]);

        self.tree
            .read_partition(key, |tree| candidates_in(tree, entity_geometry, &envelope))
            .unwrap_or_default()
    }

    /// Insert or replace one subscription's entry under one partition.
    ///
    /// Only this partition's copy of the subscription is replaced, not every
    /// partition it appears in: a subscription with several `entities`
    /// selectors is indexed once per selector, so a global removal here would
    /// leave only the last selector's partition standing.
    pub fn insert(&self, key: SpatialKey, entry: SubEntry) {
        // A subscription carries exactly one reference geometry, so it occupies
        // a partition once (clause 5.8.1 admits a single `geoQ`).
        let id = entry.id().clone(); // clone: the back-index owns the identifier
        self.tree.insert_all(key, id, vec![entry]);
    }

    /// Remove every entry belonging to a subscription, across every partition.
    pub fn remove(&self, subscription_id: &Identifier) {
        self.tree.remove(subscription_id);
    }
}

/// Every subscription in one partition's tree whose geo filter the entity
/// geometry, projected onto the horizontal plane, satisfies.
fn candidates_in(tree: &RTree<SubEntry>, entity_geometry: &PlanarGeometry, envelope: &AABB<[f64; 2]>) -> Vec<Identifier> {
    let mut matched = Vec::new();
    for entry in tree.locate_in_envelope_intersecting(*envelope) {
        if matches(&entry.georel, entity_geometry, &entry.ref_geometry) {
            matched.push(entry.subscription_id.clone()); // clone: the answer outlives the tree borrow
        }
    }

    // `disjoint` and a `near` with only a `minDistance` match entries the
    // envelope test discards: `disjoint` matches away from the reference, and
    // a `minDistance` alone arbitrarily far from it. Both need the whole
    // partition, mirroring the entity index's `candidates_for`. A `near` with a
    // `maxDistance` does not: its envelope covers every point within that
    // distance. The scan only touches entries the envelope pass missed, so no
    // entry is considered twice.
    for entry in tree {
        if matches!(entry.georel, GeoRel::Disjoint | GeoRel::Near { max_distance: None, .. })
            && !entry.envelope.intersects(envelope)
            && matches(&entry.georel, entity_geometry, &entry.ref_geometry)
        {
            matched.push(entry.subscription_id.clone()); // clone: same
        }
    }

    matched
}

#[cfg(test)]
mod tests {
    use crate::{
        key::SpatialKey,
        subscription::{entry::SubEntry, index::SpatialSubscriptionIndex},
    };
    use andromeda_ngsi_ld_geometry::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        linear_ring::LinearRing,
        planar::PlanarGeometry,
        position::Position,
        positive_distance::PositiveDistance,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};
    use std::str::FromStr;

    fn uri(id: &str) -> Identifier {
        format!("urn:ngsi-ld:Subscription:{id}").parse().expect("a valid subscription URI")
    }

    fn key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        )
    }

    fn observation_space_key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            "https://uri.etsi.org/ngsi-ld/observationSpace".parse().expect("valid attribute name"),
        )
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

    fn entry(id: &str, reference: &GeometryForm, georel: GeoRel) -> SubEntry {
        SubEntry::new(uri(id), reference, georel).expect("the reference geometry has a bounding rectangle")
    }

    fn candidates_of(index: &SpatialSubscriptionIndex, key: &SpatialKey, entity: &GeometryForm) -> Vec<Identifier> {
        index.candidates(key, &PlanarGeometry::project(entity))
    }

    #[test]
    fn an_entity_inside_a_subscriptions_area_makes_it_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-1", &square(0.0, 0.0, 1.0), GeoRel::Within));
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(0.5, 0.5)), vec![uri("sub-1")]);
    }

    #[test]
    fn an_entity_beyond_the_maximum_distance_is_not_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(
            key(),
            entry(
                "sub-near",
                &point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("100").expect("positive distance")),
                    min_distance: None,
                },
            ),
        );
        assert_eq!(candidates_of(&index, &key(), &point(10.0, 10.0)), [] as [Identifier; 0]);
    }

    #[test]
    fn the_near_envelope_is_grown_far_enough_to_cover_the_maximum_distance() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(
            key(),
            entry(
                "sub-5km",
                &point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("5000").expect("positive distance")),
                    min_distance: None,
                },
            ),
        );

        // ~4 km east is 0.036 degrees at the equator; ~6 km east is 0.054.
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(0.036, 0.0)), vec![uri("sub-5km")]);
        assert_eq!(candidates_of(&index, &key(), &point(0.054, 0.0)), [] as [Identifier; 0]);
    }

    #[test]
    fn a_removed_subscription_stops_being_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-1", &square(0.0, 0.0, 1.0), GeoRel::Within));
        index.remove(&uri("sub-1"));
        assert_eq!(candidates_of(&index, &key(), &point(0.0, 0.0)), [] as [Identifier; 0]);
    }

    #[test]
    fn a_subscription_on_one_geoproperty_is_not_a_candidate_for_another() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(observation_space_key(), entry("sub-obs", &square(0.0, 0.0, 1.0), GeoRel::Within));
        assert_eq!(candidates_of(&index, &key(), &point(0.0, 0.0)), [] as [Identifier; 0]);
    }

    #[test]
    fn a_disjoint_subscription_is_found_even_though_its_envelope_misses_the_entity() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-disjoint", &square(0.0, 0.0, 1.0), GeoRel::Disjoint));
        // Far outside the subscription's envelope, which is exactly when a
        // `disjoint` filter matches.
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(50.0, 50.0)), vec![uri("sub-disjoint")]);
    }

    #[test]
    fn a_two_selector_subscription_is_found_under_both_of_its_partitions() {
        // One subscription indexed once per `entities` selector; the second
        // insert must not wipe the first partition's entry (per-key removal).
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-multi", &square(0.0, 0.0, 1.0), GeoRel::Within));
        index.insert(observation_space_key(), entry("sub-multi", &square(0.0, 0.0, 1.0), GeoRel::Within));

        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(0.0, 0.0)), vec![uri("sub-multi")]);
        pretty_assertions::assert_eq!(candidates_of(&index, &observation_space_key(), &point(0.0, 0.0)), vec![uri("sub-multi")]);
    }

    #[test]
    fn a_min_distance_only_near_subscription_is_found_for_a_far_entity() {
        // Clause 4.10: a `near` with only a `minDistance` matches entities at
        // least that far away, which no envelope can bound, so the partition is
        // scanned whole. The bare reference envelope (a point) would otherwise
        // miss the far entity entirely.
        let index = SpatialSubscriptionIndex::new();
        index.insert(
            key(),
            entry(
                "sub-min",
                &point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: None,
                    min_distance: Some(PositiveDistance::from_str("300").expect("positive distance")),
                },
            ),
        );
        // ~2 km east at the equator, well beyond the 300 m minimum.
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(0.018, 0.0)), vec![uri("sub-min")]);
    }

    #[test]
    fn the_near_envelope_grows_the_longitude_axis_wider_at_high_latitude() {
        // At 60N a metre spans ~twice the longitude it does at the equator, so
        // the east-west grow must be ~2x the north-south grow rather than equal
        // to it, or the reference falls outside a too-narrow envelope.
        let entry = entry(
            "sub-north",
            &point(0.0, 60.0),
            GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("5000").expect("positive distance")),
                min_distance: None,
            },
        );
        let width = entry.envelope.upper()[0] - entry.envelope.lower()[0];
        let height = entry.envelope.upper()[1] - entry.envelope.lower()[1];
        assert!(width > height * 1.9, "longitude grow {width} should be ~2x latitude grow {height} at 60N");
    }

    fn near(max_distance: Option<&str>, min_distance: Option<&str>) -> GeoRel {
        GeoRel::Near {
            max_distance: max_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: min_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
        }
    }

    /// Bremen, the reference point the minimum-distance fixtures sit around.
    fn bremen() -> GeometryForm {
        point(8.8072, 53.0758)
    }

    /// A square whose west wall is 100.06 m east of Bremen and whose centroid
    /// is 170.10 m east of it.
    fn east_square() -> GeometryForm {
        GeometryForm::Polygon(PolygonRings::new(vec![
            LinearRing::try_new(
                [
                    (8.808_693, 53.075_171),
                    (8.810_783, 53.075_171),
                    (8.810_783, 53.076_429),
                    (8.808_693, 53.076_429),
                    (8.808_693, 53.075_171),
                ]
                .into_iter()
                .map(|(x, y)| Position::try_new(x, y, None).expect("a test position lies on the CRS84 axes"))
                .collect(),
            )
            .expect("a test square is a closed ring"),
        ]))
    }

    /// Clause 4.10 `near` measures the minimum distance, so a building whose
    /// nearest wall is in range is notified for though its centroid is not.
    #[test]
    fn a_near_subscription_matches_a_polygon_whose_nearest_wall_is_in_range() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-max", &bremen(), near(Some("150"), None)));
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &east_square()), vec![uri("sub-max")]);
    }

    #[test]
    fn a_min_distance_subscription_ignores_a_polygon_whose_nearest_wall_is_too_close() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-min", &bremen(), near(None, Some("150"))));
        assert_eq!(candidates_of(&index, &key(), &east_square()), [] as [Identifier; 0]);
    }

    #[test]
    fn a_min_distance_only_subscription_is_still_found_far_away() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-far", &point(0.0, 0.0), near(None, Some("1000"))));
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(50.0, 50.0)), vec![uri("sub-far")]);
    }

    #[test]
    fn a_near_subscription_across_the_antimeridian_matches_the_other_side() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-fiji", &point(179.999, -17.0), near(Some("300"), None)));
        pretty_assertions::assert_eq!(candidates_of(&index, &key(), &point(-179.9995, -17.0)), vec![uri("sub-fiji")]);
    }
}
