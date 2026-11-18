//! The reference geometry a distance ordering measures from (clause 4.23, clause 5.2.43).

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use serde::{Serialize, Serializer};

/// A reference geometry for distance-based `orderBy` terms (clauses 4.23 and 5.2.43), built from `orderFrom` and `orderGeometry`.
///
/// When present, distance terms measure from it instead of from the geo-query geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderingGeometry {
    /// The reference geometry the distance predicates measure from.
    pub geometry: Geometry,
}

impl OrderingGeometry {
    /// Wrap a geometry into an ordering reference.
    #[must_use]
    pub const fn new(geometry: Geometry) -> OrderingGeometry {
        OrderingGeometry { geometry }
    }
}

// `Geometry` already serializes as the exact RFC 7946 object whose members Table 5.2.43-1 embeds.
impl Serialize for OrderingGeometry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.geometry.serialize(serializer)
    }
}
