//! What a request-side geo-query is admitted under.

use crate::geo::near_distance_band::NearDistanceBand;
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;

/// The broker's terms for a geo-query a request writes: how large its
/// reference geometry may be (clause 5.5.6) and whether a `near` may carry
/// both distances.
#[derive(Debug, Clone, Copy)]
pub struct GeoQueryAdmission {
    /// The limits the reference geometry stays within.
    pub limits: GeometryCensusLimits,
    /// Whether a `near` may carry both `maxDistance` and `minDistance`.
    pub near_band: NearDistanceBand,
}

impl GeoQueryAdmission {
    /// The terms of `limits` and `near_band`.
    ///
    /// Written out because `derive_more::Constructor` spells its fields
    /// redundantly, which clippy refuses.
    #[must_use]
    pub const fn new(limits: GeometryCensusLimits, near_band: NearDistanceBand) -> GeoQueryAdmission {
        GeoQueryAdmission { limits, near_band }
    }
}
