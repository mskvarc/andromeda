//! One entity's geometry, as the tree holds it.

use crate::partitioned_rtree::Indexed;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use rstar::{AABB, RTreeObject};
use urn_rs::Urn;

/// A single entity's geometry, indexed under one
/// [`SpatialKey`](crate::key::SpatialKey).
///
/// The envelope is computed once at construction rather than per query: the
/// R-tree asks for it on every comparison, and recomputing a bounding
/// rectangle from a polygon each time is what the prefilter exists to avoid.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialEntry {
    /// URN of the entity whose geoproperty this entry represents.
    pub entity_id: Urn,
    /// The entity geometry.
    pub geometry: Geometry,
    /// Axis-aligned bounding box of the geometry.
    pub envelope: AABB<[f64; 2]>,
}

impl SpatialEntry {
    /// An entry for one entity, or `None` if the geometry has no bounding
    /// rectangle.
    ///
    /// An empty collection has no rectangle and therefore no way to be
    /// prefiltered; refusing to index it is what keeps the tree's answer and
    /// the exact predicate's answer the same.
    pub fn new(entity_id: Urn, geometry: Geometry) -> Option<SpatialEntry> {
        let rect = geometry.bounding_rect()?;
        let envelope = AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]);
        Some(SpatialEntry { entity_id, geometry, envelope })
    }
}

impl Indexed for SpatialEntry {
    type Id = Urn;

    fn id(&self) -> &Urn {
        &self.entity_id
    }
}

impl RTreeObject for SpatialEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}
