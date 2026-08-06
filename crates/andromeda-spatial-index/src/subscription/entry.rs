//! One subscription's geo filter, as the tree holds it.

use andromeda_ngsi_ld_geometry::{distance::metres_to_degrees_coarse, geo_rel::GeoRel};
use geo::algorithm::BoundingRect;
use rstar::{AABB, RTreeObject};

/// A single subscription's geo filter.
///
/// For a `near` filter the envelope is the reference geometry's bounding
/// rectangle grown by a coarse metres-to-degrees conversion of `maxDistance`.
/// The expansion is deliberately over-selective — roughly 1.1% larger than
/// needed at the equator and more at higher latitudes — because the exact
/// Haversine check in [`SpatialSubscriptionIndex::candidates`](crate::subscription::index::SpatialSubscriptionIndex::candidates)
/// rejects the false positives and no check can recover a false negative.
#[derive(Debug, Clone, PartialEq)]
pub struct SubEntry {
    /// Subscription identifier.
    pub subscription_id: String,
    /// The reference geometry the subscription filters against.
    pub ref_geometry: geo::Geometry<f64>,
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
    pub fn new(subscription_id: String, ref_geometry: geo::Geometry<f64>, georel: GeoRel) -> Option<SubEntry> {
        let rect = ref_geometry.bounding_rect()?;
        let mut min = [rect.min().x, rect.min().y];
        let mut max = [rect.max().x, rect.max().y];

        if let GeoRel::Near {
            max_distance: Some(metres), ..
        } = &georel
        {
            let degrees = metres_to_degrees_coarse(*metres);
            min[0] -= degrees;
            min[1] -= degrees;
            max[0] += degrees;
            max[1] += degrees;
        }

        Some(SubEntry {
            subscription_id,
            ref_geometry,
            georel,
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
