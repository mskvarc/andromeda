//! Subscription-side R-tree cache.
//!
//! Inverse of [`crate::service::spatial::entity_index::SpatialEntityIndex`]:
//! indexes every subscription's reference geometry so a single entity change
//! can be matched against O(log n) candidate subscriptions instead of every
//! geo-enabled subscription in the cache.

use crate::service::{compiled_subscription::CompiledGeoQuery, spatial::SpatialKey};
use andromeda_geometry::{distance::metres_to_degrees_coarse, geo_rel::GeoRel, relation::matches_geo};
use dashmap::DashMap;
use geo::algorithm::BoundingRect;
use parking_lot::RwLock;
use rstar::{AABB, Envelope, RTree, RTreeObject};
use std::sync::Arc;

/// A single subscription's geo filter, stored in the subscription R-tree.
///
/// The envelope is expanded from the reference geometry's bounding rectangle
/// for `near` subscriptions by a coarse conversion of `max_distance` metres to
/// degrees. The expansion is intentionally over-selective (~1.1% larger than
/// strictly necessary near the equator, more at higher latitudes); the exact
/// Haversine check inside [`SpatialSubscriptionIndex::candidates`] rejects
/// false positives.
#[derive(Debug, Clone, PartialEq)]
pub struct SubEntry {
    /// Subscription identifier (opaque URI string).
    pub subscription_id: String,
    /// Reference geometry parsed from `geoQ.coordinates` at compile time.
    pub ref_geometry: geo::Geometry<f64>,
    /// Compiled georel operator (copied from `CompiledGeoQuery::georel`).
    pub georel: GeoRel,
    /// AABB used by the R-tree for candidate pruning.
    pub envelope: AABB<[f64; 2]>,
}

impl SubEntry {
    /// Build a sub-entry from a compiled geo-query.
    pub fn from_compiled(subscription_id: String, compiled: &CompiledGeoQuery) -> Option<SubEntry> {
        let rect = compiled.ref_geometry.bounding_rect()?;
        let mut min = [rect.min().x, rect.min().y];
        let mut max = [rect.max().x, rect.max().y];

        if let GeoRel::Near { max_distance: Some(m), .. } = &compiled.georel {
            let d = metres_to_degrees_coarse(*m);
            min[0] -= d;
            min[1] -= d;
            max[0] += d;
            max[1] += d;
        }

        Some(SubEntry {
            subscription_id,
            ref_geometry: compiled.ref_geometry.clone(),
            georel: compiled.georel.clone(),
            envelope: AABB::from_corners(min, max),
        })
    }
}

impl RTreeObject for SubEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}

/// Thread-safe R-tree cache of subscription reference geometries.
#[derive(Debug, Default)]
pub struct SpatialSubscriptionIndex {
    buckets: DashMap<SpatialKey, Arc<RwLock<RTree<SubEntry>>>>,
    by_sub: DashMap<String, Vec<SpatialKey>>,
}

impl SpatialSubscriptionIndex {
    /// Create an empty index.
    pub fn new() -> SpatialSubscriptionIndex {
        SpatialSubscriptionIndex::default()
    }

    /// Return every subscription ID whose geo filter plausibly matches the
    /// given entity geometry, within the partition for `key`.
    ///
    /// Results are filtered to exact georel semantics (using
    /// `geo::Contains`/`Intersects`/Haversine) so the caller does not need a
    /// second pass for bbox-only candidates.
    pub fn candidates(&self, key: &SpatialKey, entity_geometry: &geo::Geometry<f64>) -> Vec<String> {
        let Some(bucket) = self.buckets.get(key) else {
            return Vec::new();
        };
        let tree = bucket.value().read();
        let Some(entity_rect) = entity_geometry.bounding_rect() else {
            return Vec::new();
        };
        let env = AABB::from_corners([entity_rect.min().x, entity_rect.min().y], [entity_rect.max().x, entity_rect.max().y]);

        let mut out = Vec::new();
        for entry in tree.locate_in_envelope_intersecting(&env) {
            if matches_geo(&entry.georel, entity_geometry, &entry.ref_geometry) {
                out.push(entry.subscription_id.clone());
            }
        }

        // Disjoint subscriptions must also consider entries outside the bbox;
        // scan fully in that case (rare — most geoQ filters use positive
        // predicates).
        for entry in tree.iter() {
            if matches!(entry.georel, GeoRel::Disjoint) && !entry.envelope.intersects(&env) && matches_geo(&entry.georel, entity_geometry, &entry.ref_geometry)
            {
                out.push(entry.subscription_id.clone());
            }
        }

        out
    }

    /// Insert or replace the entry for a subscription under `key`.
    pub fn insert(&self, key: SpatialKey, entry: SubEntry) {
        self.remove(&entry.subscription_id);

        let sub_id = entry.subscription_id.clone();
        let bucket = self.buckets.entry(key.clone()).or_insert_with(|| Arc::new(RwLock::new(RTree::new())));
        bucket.value().write().insert(entry);

        self.by_sub.entry(sub_id).or_default().push(key);
    }

    /// Remove every entry belonging to `subscription_id`, across every
    /// partition.
    pub fn remove(&self, subscription_id: &str) {
        let Some((_, keys)) = self.by_sub.remove(subscription_id) else {
            return;
        };
        for key in keys {
            let Some(bucket) = self.buckets.get(&key) else {
                continue;
            };
            let mut tree = bucket.value().write();
            let to_remove: Vec<SubEntry> = tree.iter().filter(|e| e.subscription_id == subscription_id).cloned().collect();
            for entry in to_remove {
                tree.remove(&entry);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenancy::types::TenantRef;
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

    fn compiled(ref_geom: geo::Geometry<f64>, georel: GeoRel) -> CompiledGeoQuery {
        CompiledGeoQuery {
            ref_geometry: ref_geom,
            georel,
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        }
    }

    #[test]
    fn candidates_returns_sub_for_entity_in_subscription_area() {
        let idx = SpatialSubscriptionIndex::new();
        let entry = SubEntry::from_compiled("sub-1".to_string(), &compiled(square(0.0, 0.0, 1.0), GeoRel::Within)).expect("bbox");
        idx.insert(key(), entry);

        let got = idx.candidates(&key(), &point(0.5, 0.5));
        assert_eq!(got, vec!["sub-1".to_string()]);
    }

    #[test]
    fn candidates_excludes_far_sub() {
        let idx = SpatialSubscriptionIndex::new();
        let entry = SubEntry::from_compiled(
            "sub-near".to_string(),
            &compiled(
                point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: Some(100.0),
                    min_distance: None,
                },
            ),
        )
        .expect("bbox");
        idx.insert(key(), entry);

        let got = idx.candidates(&key(), &point(10.0, 10.0));
        assert!(got.is_empty());
    }

    #[test]
    fn near_envelope_expansion_covers_max_distance() {
        let idx = SpatialSubscriptionIndex::new();
        let entry = SubEntry::from_compiled(
            "sub-5km".to_string(),
            &compiled(
                point(0.0, 0.0),
                GeoRel::Near {
                    max_distance: Some(5_000.0),
                    min_distance: None,
                },
            ),
        )
        .expect("bbox");
        idx.insert(key(), entry);

        // ~4 km east (0.036 deg at equator)
        let inside = point(0.036, 0.0);
        // ~6 km east
        let outside = point(0.054, 0.0);

        assert_eq!(idx.candidates(&key(), &inside), vec!["sub-5km".to_string()]);
        assert!(idx.candidates(&key(), &outside).is_empty());
    }

    #[test]
    fn remove_subscription_clears_bucket() {
        let idx = SpatialSubscriptionIndex::new();
        let entry = SubEntry::from_compiled("sub-1".to_string(), &compiled(square(0.0, 0.0, 1.0), GeoRel::Within)).expect("bbox");
        idx.insert(key(), entry);
        idx.remove("sub-1");

        let got = idx.candidates(&key(), &point(0.0, 0.0));
        assert!(got.is_empty());
    }

    #[test]
    fn different_geoproperty_isolated() {
        let idx = SpatialSubscriptionIndex::new();
        let entry = SubEntry::from_compiled("sub-obs".to_string(), &compiled(square(0.0, 0.0, 1.0), GeoRel::Within)).expect("bbox");
        idx.insert(observation_space_key(), entry);

        // Query under the `location` key must not see the `observationSpace` sub.
        let got = idx.candidates(&key(), &point(0.0, 0.0));
        assert!(got.is_empty());
    }
}
