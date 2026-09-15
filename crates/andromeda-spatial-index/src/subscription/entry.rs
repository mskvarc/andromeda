//! One subscription's geo filter, as the tree holds it.

use crate::partitioned_rtree::Indexed;
use andromeda_ngsi_ld_geometry::{distance::grow_lonlat_bounds_metres, geo_rel::GeoRel, geometry::Geometry};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use rstar::{AABB, RTreeObject};

/// A single subscription's geo filter.
///
/// For a `near` filter with a `maxDistance` the envelope is the reference
/// geometry's bounding rectangle grown by a metres-to-degrees conversion of
/// that distance: the latitude axis by the near-constant degrees-per-metre, the
/// longitude axis by the wider `1 / cos(latitude)` amount an east-west metre
/// spans away from the equator. The expansion is deliberately over-selective —
/// it never under-covers — because the exact Haversine check in
/// [`SpatialSubscriptionIndex::candidates`](crate::subscription::index::SpatialSubscriptionIndex::candidates)
/// rejects the false positives, and a `near` with only a `minDistance` (or one
/// whose grown envelope still falls short) is caught by that method's
/// whole-partition rescue, so no check ever has to recover a false negative.
#[derive(Debug, Clone, PartialEq)]
pub struct SubEntry {
    /// Subscription identifier.
    pub subscription_id: Uri,
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
    pub fn new(subscription_id: Uri, ref_geometry: Geometry, georel: GeoRel) -> Option<SubEntry> {
        let rect = ref_geometry.bounding_rect()?;
        let mut min = [rect.min().x, rect.min().y];
        let mut max = [rect.max().x, rect.max().y];

        if let GeoRel::Near {
            max_distance: Some(metres), ..
        } = &georel
        {
            // Grow the reference box outward by `maxDistance`, widening the
            // east-west axis correctly with latitude via a great-circle
            // destination rather than a flat degrees-per-metre factor.
            (min, max) = grow_lonlat_bounds_metres(min, max, metres.metres());
        }

        Some(SubEntry {
            subscription_id,
            ref_geometry,
            georel,
            envelope: AABB::from_corners(min, max),
        })
    }
}

impl Indexed for SubEntry {
    type Id = Uri;

    fn id(&self) -> &Uri {
        &self.subscription_id
    }
}

impl RTreeObject for SubEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}
