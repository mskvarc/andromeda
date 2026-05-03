//! Entity-side R-tree cache.
//!
//! Partitioned by [`SpatialKey`]; each partition holds a `RTree<SpatialEntry>`
//! storing the full entity geometry. [`SpatialEntityIndex::query`] runs the
//! exact `georel` predicate inline using the `geo` crate, matching the
//! semantics of [`crate::service::subscription_matching::matches_compiled_geo_query`].

use crate::{
    domain::geometry::Wkb,
    query::compiled::GeoRel,
    service::spatial::{SpatialKey, distance::haversine_distance_m},
};
use dashmap::DashMap;
use geo::algorithm::{Contains, Intersects};
use parking_lot::RwLock;
use rstar::{AABB, PointDistance, RTree, RTreeObject};
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

/// A single entity's geometry, indexed under one [`SpatialKey`].
///
/// The geometry is held as canonical WKB; the R-tree prefilter uses the cached
/// envelope so no decode is paid for entries that fail the AABB test. Only
/// surviving candidates pay the WKB→`geo::Geometry` decode inside the exact
/// predicate step.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialEntry {
    /// URN of the entity whose geoproperty this entry represents.
    pub entity_id: Urn,
    /// Canonical WKB of the entity geometry.
    pub geometry_wkb: Wkb,
    /// Axis-aligned bounding box of the decoded geometry (cached for rstar).
    pub envelope: AABB<[f64; 2]>,
}

impl SpatialEntry {
    /// Build a new entry, decoding once to compute the AABB and then retaining only WKB.
    ///
    /// Returns `None` if the geometry has no bounding rectangle (empty
    /// collections or degenerate inputs).
    pub fn new(entity_id: Urn, geometry_wkb: Wkb) -> Option<SpatialEntry> {
        let rect = geometry_wkb.bounding_rect()?;
        let envelope = AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]);
        Some(SpatialEntry {
            entity_id,
            geometry_wkb,
            envelope,
        })
    }
}

impl RTreeObject for SpatialEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}

impl PointDistance for SpatialEntry {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        self.envelope.distance_2(point)
    }
}

/// Thread-safe R-tree cache of entity geometries.
///
/// Each `(tenant, type, geoproperty)` bucket has its own tree under an
/// `Arc<RwLock<_>>`. A dedicated secondary map tracks every key under which a
/// given entity was last indexed so `remove` is O(k) in the number of
/// geoproperties per entity (typically 1-2) rather than scanning every bucket.
#[derive(Debug, Default)]
pub struct SpatialEntityIndex {
    buckets: DashMap<SpatialKey, Arc<RwLock<RTree<SpatialEntry>>>>,
    by_entity: DashMap<Urn, Vec<SpatialKey>>,
}

impl SpatialEntityIndex {
    /// Create an empty index.
    pub fn new() -> SpatialEntityIndex {
        SpatialEntityIndex::default()
    }

    /// Run a `georel` query against the partition for `key`.
    ///
    /// Returns `Some(candidate_ids)` when the partition exists (even if
    /// empty after filtering), or `None` when the partition is cold. The
    /// caller uses `None` as a signal to fall through to the SQL path and
    /// (optionally) trigger a lazy bulk load.
    pub fn query(&self, key: &SpatialKey, georel: &GeoRel, ref_geom: &geo::Geometry<f64>) -> Option<HashSet<Urn>> {
        let bucket = self.buckets.get(key)?;
        let tree = bucket.value().read();
        let matching = evaluate(&tree, georel, ref_geom);
        Some(matching)
    }

    /// Insert or overwrite an entry for `entity_id` under `key`.
    pub fn insert(&self, key: SpatialKey, entity_id: Urn, geometry_wkb: Wkb) {
        let Some(entry) = SpatialEntry::new(entity_id.clone(), geometry_wkb) else {
            return;
        };

        self.remove_from_key(&entity_id, &key);

        let bucket = self.buckets.entry(key.clone()).or_insert_with(|| Arc::new(RwLock::new(RTree::new())));
        bucket.value().write().insert(entry);

        let mut owned_keys = self.by_entity.entry(entity_id).or_default();
        if !owned_keys.value().contains(&key) {
            owned_keys.value_mut().push(key);
        }
    }

    /// Remove every entry belonging to `entity_id`, across every partition.
    pub fn remove(&self, entity_id: &Urn) {
        let Some((_, keys)) = self.by_entity.remove(entity_id) else {
            return;
        };
        for key in keys {
            self.remove_from_key_inner(entity_id, &key);
        }
    }

    /// Replace the contents of `key`'s partition with `entries` via bulk-load.
    ///
    /// Bulk-loading yields a much better tree layout than sequential inserts
    /// and swaps atomically under the existing `Arc<RwLock<_>>`, so in-flight
    /// readers see either the old tree or the new tree but never a partial
    /// state.
    pub fn bulk_load(&self, key: SpatialKey, entries: Vec<SpatialEntry>) {
        let by_entity = &self.by_entity;
        for e in &entries {
            let mut owned = by_entity.entry(e.entity_id.clone()).or_default();
            if !owned.value().contains(&key) {
                owned.value_mut().push(key.clone());
            }
        }
        let tree = RTree::bulk_load(entries);
        let bucket = self.buckets.entry(key).or_insert_with(|| Arc::new(RwLock::new(RTree::new())));
        *bucket.value().write() = tree;
    }

    fn remove_from_key(&self, entity_id: &Urn, key: &SpatialKey) {
        self.remove_from_key_inner(entity_id, key);
        if let Some(mut owned) = self.by_entity.get_mut(entity_id) {
            owned.value_mut().retain(|k| k != key);
        }
    }

    fn remove_from_key_inner(&self, entity_id: &Urn, key: &SpatialKey) {
        let Some(bucket) = self.buckets.get(key) else {
            return;
        };
        let mut tree = bucket.value().write();
        let to_remove: Vec<SpatialEntry> = tree.iter().filter(|e| e.entity_id == *entity_id).cloned().collect();
        for entry in to_remove {
            tree.remove(&entry);
        }
    }
}

fn evaluate(tree: &RTree<SpatialEntry>, georel: &GeoRel, ref_geom: &geo::Geometry<f64>) -> HashSet<Urn> {
    let mut out = HashSet::new();

    match georel {
        GeoRel::Near { max_distance, min_distance } => {
            // Near requires scanning all entries (no bbox cutoff works for `min`).
            for entry in tree.iter() {
                let Ok(entry_geom) = entry.geometry_wkb.to_geo() else {
                    continue;
                };
                let d = haversine_distance_m(&entry_geom, ref_geom);
                let max_ok = max_distance.is_none_or(|m| d <= m);
                let min_ok = min_distance.is_none_or(|m| d >= m);
                if max_ok && min_ok {
                    out.insert(entry.entity_id.clone());
                }
            }
        }
        GeoRel::Disjoint => {
            // Disjoint = not intersects. Must consider every entry.
            for entry in tree.iter() {
                let Ok(entry_geom) = entry.geometry_wkb.to_geo() else {
                    continue;
                };
                if !entry_geom.intersects(ref_geom) {
                    out.insert(entry.entity_id.clone());
                }
            }
        }
        _ => {
            let ref_env = geometry_envelope(ref_geom);
            // AABB prefilter — no decode paid for entries that fail the envelope test.
            let candidates: Vec<&SpatialEntry> = tree.locate_in_envelope_intersecting(&ref_env).collect();
            for entry in candidates {
                let Ok(entry_geom) = entry.geometry_wkb.to_geo() else {
                    continue;
                };
                let matched = match georel {
                    GeoRel::Within => ref_geom.contains(&entry_geom),
                    GeoRel::Contains => entry_geom.contains(ref_geom),
                    GeoRel::Intersects => entry_geom.intersects(ref_geom),
                    GeoRel::Equals => entry_geom.contains(ref_geom) && ref_geom.contains(&entry_geom),
                    GeoRel::Overlaps => entry_geom.intersects(ref_geom),
                    GeoRel::Near { .. } | GeoRel::Disjoint => false,
                };
                if matched {
                    out.insert(entry.entity_id.clone());
                }
            }
        }
    }

    out
}

fn geometry_envelope(g: &geo::Geometry<f64>) -> AABB<[f64; 2]> {
    use geo::algorithm::BoundingRect;
    match g.bounding_rect() {
        Some(rect) => AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]),
        None => AABB::from_corners([f64::NEG_INFINITY, f64::NEG_INFINITY], [f64::INFINITY, f64::INFINITY]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
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

    fn urn(s: &str) -> Urn {
        s.parse().expect("valid URN")
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

    fn wkb_point(x: f64, y: f64) -> Wkb {
        Wkb::from_geo(&point(x, y)).expect("point WKB")
    }

    fn wkb_square(cx: f64, cy: f64, half: f64) -> Wkb {
        Wkb::from_geo(&square(cx, cy, half)).expect("square WKB")
    }

    #[test]
    fn insert_then_query_within_returns_id() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        idx.insert(key(), id.clone(), wkb_point(0.0, 0.0));
        let got = idx.query(&key(), &GeoRel::Within, &square(0.0, 0.0, 1.0));
        assert_eq!(got, Some(HashSet::from([id])));
    }

    #[test]
    fn insert_then_query_disjoint_returns_empty_when_overlapping() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        idx.insert(key(), id, wkb_square(0.0, 0.0, 2.0));
        let got = idx.query(&key(), &GeoRel::Disjoint, &square(0.0, 0.0, 1.0));
        assert_eq!(got, Some(HashSet::new()));
    }

    #[test]
    fn near_respects_max_distance() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        // 2 km east at the equator is ~0.01797 degrees of longitude
        idx.insert(key(), id.clone(), wkb_point(0.01797, 0.0));

        let ref_geom = point(0.0, 0.0);
        let tight = GeoRel::Near {
            max_distance: Some(1_000.0),
            min_distance: None,
        };
        let loose = GeoRel::Near {
            max_distance: Some(3_000.0),
            min_distance: None,
        };

        assert_eq!(idx.query(&key(), &tight, &ref_geom), Some(HashSet::new()));
        assert_eq!(idx.query(&key(), &loose, &ref_geom), Some(HashSet::from([id])));
    }

    #[test]
    fn near_respects_min_distance_band() {
        let idx = SpatialEntityIndex::new();
        let a = urn("urn:ngsi-ld:Vehicle:100m");
        let b = urn("urn:ngsi-ld:Vehicle:500m");
        let c = urn("urn:ngsi-ld:Vehicle:2000m");
        // ~0.001 deg = ~111 m; scale accordingly
        idx.insert(key(), a, wkb_point(0.0009, 0.0));
        idx.insert(key(), b.clone(), wkb_point(0.0045, 0.0));
        idx.insert(key(), c, wkb_point(0.018, 0.0));

        let georel = GeoRel::Near {
            max_distance: Some(1_000.0),
            min_distance: Some(300.0),
        };
        let got = idx.query(&key(), &georel, &point(0.0, 0.0));
        assert_eq!(got, Some(HashSet::from([b])));
    }

    #[test]
    fn remove_clears_entry() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        idx.insert(key(), id.clone(), wkb_point(0.0, 0.0));
        idx.remove(&id);
        let got = idx.query(&key(), &GeoRel::Intersects, &square(-10.0, -10.0, 20.0));
        assert_eq!(got, Some(HashSet::new()));
    }

    #[test]
    fn cold_key_returns_none() {
        let idx = SpatialEntityIndex::new();
        let got = idx.query(&key(), &GeoRel::Intersects, &point(0.0, 0.0));
        assert!(got.is_none());
    }

    #[test]
    fn bulk_load_equivalent_to_individual_inserts() {
        let idx_a = SpatialEntityIndex::new();
        let idx_b = SpatialEntityIndex::new();
        let mut entries = Vec::new();
        for i in 0..50 {
            let id = urn(&format!("urn:ngsi-ld:V:{i}"));
            let wkb = wkb_point(i as f64 * 0.001, 0.0);
            idx_a.insert(key(), id.clone(), wkb.clone());
            if let Some(entry) = SpatialEntry::new(id, wkb) {
                entries.push(entry);
            }
        }
        idx_b.bulk_load(key(), entries);

        let q_geom = square(0.01, 0.0, 0.005);
        let a = idx_a.query(&key(), &GeoRel::Intersects, &q_geom).unwrap_or_default();
        let b = idx_b.query(&key(), &GeoRel::Intersects, &q_geom).unwrap_or_default();
        assert_eq!(a, b);
    }

    #[test]
    fn polygon_entity_contains_point_ref() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Building:Hall");
        idx.insert(key(), id.clone(), wkb_square(0.0, 0.0, 1.0));
        let got = idx.query(&key(), &GeoRel::Contains, &point(0.5, 0.5));
        assert_eq!(got, Some(HashSet::from([id])));
    }

    #[test]
    fn equals_requires_bidirectional_containment() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        idx.insert(key(), id.clone(), wkb_square(0.0, 0.0, 1.0));

        let same = square(0.0, 0.0, 1.0);
        let bigger = square(0.0, 0.0, 2.0);

        assert_eq!(idx.query(&key(), &GeoRel::Equals, &same), Some(HashSet::from([id])));
        assert_eq!(idx.query(&key(), &GeoRel::Equals, &bigger), Some(HashSet::new()));
    }

    #[test]
    fn overlaps_matches_partial_intersection() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Region:A");
        idx.insert(key(), id.clone(), wkb_square(0.0, 0.0, 1.0));
        let got = idx.query(&key(), &GeoRel::Overlaps, &square(0.5, 0.5, 1.0));
        assert_eq!(got, Some(HashSet::from([id])));
    }

    #[test]
    fn insert_twice_overwrites_previous_geometry() {
        let idx = SpatialEntityIndex::new();
        let id = urn("urn:ngsi-ld:Vehicle:A");
        idx.insert(key(), id.clone(), wkb_point(0.0, 0.0));
        idx.insert(key(), id.clone(), wkb_point(10.0, 10.0));

        let near_origin = idx.query(&key(), &GeoRel::Intersects, &square(0.0, 0.0, 0.5)).unwrap_or_default();
        let near_ten = idx.query(&key(), &GeoRel::Intersects, &square(10.0, 10.0, 0.5)).unwrap_or_default();

        assert!(near_origin.is_empty());
        assert_eq!(near_ten, HashSet::from([id]));
    }
}
