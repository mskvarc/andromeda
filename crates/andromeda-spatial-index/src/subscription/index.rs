//! The R-tree of subscription reference geometries.

use crate::{key::SpatialKey, subscription::entry::SubEntry};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, relation::matches_geo};
use dashmap::DashMap;
use geo::algorithm::BoundingRect;
use parking_lot::RwLock;
use rstar::{AABB, Envelope, RTree};
use std::sync::Arc;

/// Thread-safe R-tree cache of subscription reference geometries.
///
/// The inverse of the entity index: one entity change is matched against
/// O(log n) candidate subscriptions instead of every geo-enabled subscription
/// the broker holds.
#[derive(Debug, Default)]
pub struct SpatialSubscriptionIndex {
    /// One tree per partition.
    buckets: DashMap<SpatialKey, Arc<RwLock<RTree<SubEntry>>>>,
    /// Which partitions each subscription appears in.
    by_sub: DashMap<String, Vec<SpatialKey>>,
}

impl SpatialSubscriptionIndex {
    /// An empty index.
    pub fn new() -> SpatialSubscriptionIndex {
        SpatialSubscriptionIndex::default()
    }

    /// Every subscription in one partition whose geo filter the given entity
    /// geometry satisfies.
    ///
    /// Already exact: each candidate the tree admits is put through the clause
    /// 4.10 predicate before it is returned, so the caller needs no second
    /// pass.
    pub fn candidates(&self, key: &SpatialKey, entity_geometry: &geo::Geometry<f64>) -> Vec<String> {
        let Some(bucket) = self.buckets.get(key) else {
            return Vec::new();
        };
        let tree = bucket.value().read();
        let Some(rect) = entity_geometry.bounding_rect() else {
            return Vec::new();
        };
        let envelope = AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]);

        let mut matched = Vec::new();
        for entry in tree.locate_in_envelope_intersecting(&envelope) {
            if matches_geo(&entry.georel, entity_geometry, &entry.ref_geometry) {
                matched.push(entry.subscription_id.clone()); // clone: the answer outlives the tree borrow
            }
        }

        // A `disjoint` subscription matches precisely the entries the envelope
        // test discards, so it has to be looked for outside the envelope too.
        // Rare enough to be worth a full scan: most `geoQ` filters are
        // positive.
        for entry in tree.iter() {
            if matches!(entry.georel, GeoRel::Disjoint)
                && !entry.envelope.intersects(&envelope)
                && matches_geo(&entry.georel, entity_geometry, &entry.ref_geometry)
            {
                matched.push(entry.subscription_id.clone()); // clone: same
            }
        }

        matched
    }

    /// Insert or replace one subscription's entry under one partition.
    pub fn insert(&self, key: SpatialKey, entry: SubEntry) {
        self.remove(&entry.subscription_id);

        let subscription_id = entry.subscription_id.clone(); // clone: the ownership record outlives the entry
        let bucket = self.buckets.entry(key.clone()).or_insert_with(|| Arc::new(RwLock::new(RTree::new()))); // clone: bucket key and ownership record are separate
        bucket.value().write().insert(entry);

        self.by_sub.entry(subscription_id).or_default().push(key);
    }

    /// Remove every entry belonging to a subscription, across every partition.
    pub fn remove(&self, subscription_id: &str) {
        let Some((_, keys)) = self.by_sub.remove(subscription_id) else {
            return;
        };
        for key in keys {
            let Some(bucket) = self.buckets.get(&key) else {
                continue;
            };
            let mut tree = bucket.value().write();
            // clone: `RTree::remove` takes the entry by reference and the borrow
            // of the tree it came from cannot outlive the removal
            let to_remove: Vec<SubEntry> = tree.iter().filter(|entry| entry.subscription_id == subscription_id).cloned().collect();
            for entry in to_remove {
                tree.remove(&entry);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        key::SpatialKey,
        subscription::{entry::SubEntry, index::SpatialSubscriptionIndex},
    };
    use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use geo::{Coord, LineString, Point, Polygon};
    use iri_rs::{IriBuf, iri};

    fn key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/location")),
        )
    }

    fn observation_space_key() -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::from(iri!("https://example.org/Vehicle")),
            IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/observationSpace")),
        )
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

    fn entry(id: &str, reference: geo::Geometry<f64>, georel: GeoRel) -> SubEntry {
        SubEntry::new(id.to_string(), reference, georel).expect("the reference geometry has a bounding rectangle")
    }

    #[test]
    fn an_entity_inside_a_subscriptions_area_makes_it_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-1", square(0.0, 0.0, 1.0), GeoRel::Within));
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(0.5, 0.5)), vec!["sub-1".to_string()]);
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
        assert!(index.candidates(&key(), &point(10.0, 10.0)).is_empty());
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
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(0.036, 0.0)), vec!["sub-5km".to_string()]);
        assert!(index.candidates(&key(), &point(0.054, 0.0)).is_empty());
    }

    #[test]
    fn a_removed_subscription_stops_being_a_candidate() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-1", square(0.0, 0.0, 1.0), GeoRel::Within));
        index.remove("sub-1");
        assert!(index.candidates(&key(), &point(0.0, 0.0)).is_empty());
    }

    #[test]
    fn a_subscription_on_one_geoproperty_is_not_a_candidate_for_another() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(observation_space_key(), entry("sub-obs", square(0.0, 0.0, 1.0), GeoRel::Within));
        assert!(index.candidates(&key(), &point(0.0, 0.0)).is_empty());
    }

    #[test]
    fn a_disjoint_subscription_is_found_even_though_its_envelope_misses_the_entity() {
        let index = SpatialSubscriptionIndex::new();
        index.insert(key(), entry("sub-disjoint", square(0.0, 0.0, 1.0), GeoRel::Disjoint));
        // Far outside the subscription's envelope, which is exactly when a
        // `disjoint` filter matches.
        pretty_assertions::assert_eq!(index.candidates(&key(), &point(50.0, 50.0)), vec!["sub-disjoint".to_string()]);
    }
}
