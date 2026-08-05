//! The reference geometry a distance ordering measures from (clause 4.23, clause 5.2.43).

use andromeda_ngsi_ld_geometry::geometry::Geometry;

/// A reference geometry for distance-based ordering (NGSI-LD S4.23, S5.2.43).
///
/// Constructed from the `orderFrom` and `orderGeometry` query parameters.
/// When present, distance-based `orderBy` terms use this geometry as the
/// reference point instead of the geo-query geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderingGeometry {
    /// The reference geometry the distance predicates measure from.
    pub geometry: Geometry,
}

impl OrderingGeometry {
    /// Wrap a geometry into an ordering reference.
    pub const fn new(geometry: Geometry) -> OrderingGeometry {
        OrderingGeometry { geometry }
    }
}
