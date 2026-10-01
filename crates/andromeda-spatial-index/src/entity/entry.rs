//! One entity's geometry, as the tree holds it.

use crate::partitioned_rtree::Indexed;
use andromeda_ngsi_ld_geometry::{geometry::Geometry, great_circle::great_circle_bounds};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use rstar::{AABB, RTreeObject};

/// A single entity's geometry, indexed under one
/// [`SpatialKey`](crate::key::SpatialKey).
///
/// The envelope is computed once at construction rather than per query: the
/// R-tree asks for it on every comparison, and recomputing a bounding
/// rectangle from a polygon each time is what the prefilter exists to avoid.
///
/// It is the geometry's great-circle bounds rather than its planar bounding
/// rectangle. `near` measures along great-circle edges, which bow past the
/// vertices of a long edge, and an entry whose box stopped at its vertices
/// would be missing from a `near` candidate set its bulge belongs in. The box
/// still contains the planar rectangle, so the planar relations lose nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialEntry {
    /// Identifier of the entity whose geoproperty this entry represents.
    ///
    /// Any clause 4.5.1 entity identifier, not only a URN: an entity the tree
    /// could not hold would be missing from a partition the query path reads
    /// as complete.
    pub entity_id: Identifier,
    /// The entity geometry.
    pub geometry: Geometry,
    /// Axis-aligned box covering the geometry read with great-circle edges.
    pub envelope: AABB<[f64; 2]>,
}

impl SpatialEntry {
    /// An entry for one entity, or `None` if the geometry has no bounding
    /// rectangle.
    ///
    /// An empty collection has no rectangle and therefore no way to be
    /// prefiltered; refusing to index it is what keeps the tree's answer and
    /// the exact predicate's answer the same.
    #[must_use]
    pub fn new(entity_id: Identifier, geometry: Geometry) -> Option<SpatialEntry> {
        let rect = great_circle_bounds(&geometry)?;
        let envelope = AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]);
        Some(SpatialEntry { entity_id, geometry, envelope })
    }
}

impl Indexed for SpatialEntry {
    type Id = Identifier;

    fn id(&self) -> &Identifier {
        &self.entity_id
    }
}

impl RTreeObject for SpatialEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}
