//! One subscription's geo filter, as the tree holds it.

use crate::partitioned_rtree::Indexed;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, great_circle::great_circle_bounds, near_envelope::NearEnvelope};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use rstar::{AABB, RTreeObject};

/// A single subscription's geo filter.
///
/// The envelope is the reference geometry's great-circle bounds and, for a
/// `near` filter with a `maxDistance`, the [`NearEnvelope`] around them: every
/// point within that distance on the WGS 84 spheroid, which is where clause
/// 4.10 `near` is measured. It never under-covers, and the clause 4.10
/// predicate in
/// [`SpatialSubscriptionIndex::candidates`](crate::subscription::index::SpatialSubscriptionIndex::candidates)
/// rejects what it over-covers. An envelope that wraps across the antimeridian
/// is held as the full longitude band, the one box an R-tree object carries. A
/// `near` with only a `minDistance` matches arbitrarily far from its reference
/// and is caught by that method's whole-partition rescue instead.
#[derive(Debug, Clone, PartialEq)]
pub struct SubEntry {
    /// Subscription identifier.
    pub subscription_id: Identifier,
    /// The reference geometry the subscription filters against.
    pub ref_geometry: Geometry,
    /// The relation the subscription asked for.
    pub georel: GeoRel,
    /// Bounding box used for candidate pruning.
    pub envelope: AABB<[f64; 2]>,
}

impl SubEntry {
    /// An entry for one subscription, or `None` if its reference geometry has
    /// no bounding rectangle.
    ///
    /// Takes the geometry and the relation rather than a compiled subscription:
    /// this crate indexes geo filters, and knowing what else a subscription
    /// carries would make an R-tree a subscription cache.
    #[must_use]
    pub fn new(subscription_id: Identifier, ref_geometry: Geometry, georel: GeoRel) -> Option<SubEntry> {
        let bounds = great_circle_bounds(&ref_geometry)?;
        let rect = match &georel {
            GeoRel::Near {
                max_distance: Some(max_distance),
                ..
            } => NearEnvelope::around(bounds, *max_distance).covering_rect(),
            GeoRel::Near { max_distance: None, .. }
            | GeoRel::Within
            | GeoRel::Contains
            | GeoRel::Intersects
            | GeoRel::Disjoint
            | GeoRel::Equals
            | GeoRel::Overlaps => bounds,
        };

        Some(SubEntry {
            subscription_id,
            ref_geometry,
            georel,
            envelope: AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]),
        })
    }
}

impl Indexed for SubEntry {
    type Id = Identifier;

    fn id(&self) -> &Identifier {
        &self.subscription_id
    }
}

impl RTreeObject for SubEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}
