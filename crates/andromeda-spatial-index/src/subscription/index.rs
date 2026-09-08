//! The subscription half of the spatial index: which subscriptions an entity
//! change is a candidate for.

use crate::{
    key::SpatialKey,
    partitioned_rtree::{Indexed, PartitionedRTree},
    subscription::entry::SubEntry,
};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, relation::matches};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use rstar::{AABB, Envelope, RTree};

/// Thread-safe R-tree cache of subscription reference geometries.
///
/// The inverse of the entity index: one entity change is matched against
/// O(log n) candidate subscriptions instead of every geo-enabled subscription
/// the broker holds. A thin reading over the shared [`PartitionedRTree`] that
/// adds only the candidate evaluation, which is where the two indexes differ:
/// each entry carries its own `georel`, so the match is applied per entry
/// rather than against one caller-supplied relation.
#[derive(Debug, Default)]
pub struct SpatialSubscriptionIndex {
    /// Subscription reference geometries, partitioned by
    /// `(tenant, type, geoproperty)`.
    tree: PartitionedRTree<SubEntry>,
}

impl SpatialSubscriptionIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> SpatialSubscriptionIndex {
        SpatialSubscriptionIndex::default()
    }

    /// Every subscription in one partition whose geo filter the given entity
    /// geometry satisfies.
    ///
    /// Already exact: each candidate the tree admits is put through the clause
    /// 4.10 predicate before it is returned, so the caller needs no second
    /// pass.
    pub fn candidates(&self, key: &SpatialKey, entity_geometry: &Geometry) -> Vec<Uri> {
        let Some(rect) = entity_geometry.bounding_rect() else {
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
    pub fn remove(&self, subscription_id: &Uri) {
        self.tree.remove(subscription_id);
    }
}

/// Every subscription in one partition's tree whose geo filter the entity
/// geometry satisfies.
fn candidates_in(tree: &RTree<SubEntry>, entity_geometry: &Geometry, envelope: &AABB<[f64; 2]>) -> Vec<Uri> {
    let mut matched = Vec::new();
    for entry in tree.locate_in_envelope_intersecting(*envelope) {
        if matches(&entry.georel, entity_geometry, &entry.ref_geometry) {
            matched.push(entry.subscription_id.clone()); // clone: the answer outlives the tree borrow
        }
    }

    // `disjoint` and every `near` variant match entries the envelope test
    // discards: `disjoint` matches away from the reference, and a `near`
    // with only a `minDistance` (or one whose grown envelope still falls
    // short) matches arbitrarily far from it. Both need the whole
    // partition, mirroring the entity index's `candidates_for`. The scan
    // only touches entries the envelope pass missed, so no entry is
    // considered twice.
    for entry in tree {
        if matches!(entry.georel, GeoRel::Disjoint | GeoRel::Near { .. })
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
        geometry::Geometry,
        position::Position,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::{IriBuf, iri};

    fn uri(id: &str) -> Uri {
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

    fn entry(id: &str, reference: Geometry, georel: GeoRel) -> SubEntry {
        SubEntry::new(uri(id), reference, georel).expect("the reference geometry has a bounding rectangle")
    }

    #[test]
    fn an_entity_inside_a_subscriptions_area_makes_it_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-1", square(0.0, 0.0, 1.0), GeoRel::Within));
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(0.5, 0.5)), vec![uri("sub-1")]);
    }

    #[test]
    fn an_entity_beyond_the_maximum_distance_is_not_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(
            key(),
            entry(
                "sub-near",
                point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: Some(100.0),
                    min_distance: None,
                },
            ),
        );
        assert_eq!(index.candidates(&key(), &point(10.0, 10.0)), [] as [andromeda_ngsi_ld_lexicon::uri::Uri; 0]);
    }

    #[test]
    fn the_near_envelope_is_grown_far_enough_to_cover_the_maximum_distance() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(
            key(),
            entry(
                "sub-5km",
                point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: Some(5_000.0),
                    min_distance: None,
                },
            ),
        );

        // ~4 km east is 0.036 degrees at the equator; ~6 km east is 0.054.
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(0.036, 0.0)), vec![uri("sub-5km")]);
        assert_eq!(index.candidates(&key(), &point(0.054, 0.0)), [] as [andromeda_ngsi_ld_lexicon::uri::Uri; 0]);
    }

    #[test]
    fn a_removed_subscription_stops_being_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-1", square(0.0, 0.0, 1.0), GeoRel::Within));
        index.remove(&uri("sub-1"));
        assert_eq!(index.candidates(&key(), &point(0.0, 0.0)), [] as [andromeda_ngsi_ld_lexicon::uri::Uri; 0]);
    }

    #[test]
    fn a_subscription_on_one_geoproperty_is_not_a_candidate_for_another() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(observation_space_key(), entry("sub-obs", square(0.0, 0.0, 1.0), GeoRel::Within));
        assert_eq!(index.candidates(&key(), &point(0.0, 0.0)), [] as [andromeda_ngsi_ld_lexicon::uri::Uri; 0]);
    }

    #[test]
    fn a_disjoint_subscription_is_found_even_though_its_envelope_misses_the_entity() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-disjoint", square(0.0, 0.0, 1.0), GeoRel::Disjoint));
        // Far outside the subscription's envelope, which is exactly when a
        // `disjoint` filter matches.
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(50.0, 50.0)), vec![uri("sub-disjoint")]);
    }

    #[test]
    fn a_two_selector_subscription_is_found_under_both_of_its_partitions() {
        // One subscription indexed once per `entities` selector; the second
        // insert must not wipe the first partition's entry (per-key removal).
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-multi", square(0.0, 0.0, 1.0), GeoRel::Within));
        index.insert(observation_space_key(), entry("sub-multi", square(0.0, 0.0, 1.0), GeoRel::Within));

        pretty_assertions::assert_eq!(index.candidates(&key(), &point(0.0, 0.0)), vec![uri("sub-multi")]);
        pretty_assertions::assert_eq!(index.candidates(&observation_space_key(), &point(0.0, 0.0)), vec![uri("sub-multi")]);
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
                point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: None,
                    min_distance: Some(300.0),
                },
            ),
        );
        // ~2 km east at the equator, well beyond the 300 m minimum.
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(0.018, 0.0)), vec![uri("sub-min")]);
    }

    #[test]
    fn the_near_envelope_grows_the_longitude_axis_wider_at_high_latitude() {
        // At 60N a metre spans ~twice the longitude it does at the equator, so
        // the east-west grow must be ~2x the north-south grow rather than equal
        // to it, or the reference falls outside a too-narrow envelope.
        let entry = entry(
            "sub-north",
            point(0.0, 60.0),
            GeoRel::Near {
                max_distance: Some(5_000.0),
                min_distance: None,
            },
        );
        let width = entry.envelope.upper()[0] - entry.envelope.lower()[0];
        let height = entry.envelope.upper()[1] - entry.envelope.lower()[1];
        assert!(width > height * 1.9, "longitude grow {width} should be ~2x latitude grow {height} at 60N");
    }
}
