//! The reference geometry a distance ordering measures from (clause 4.23, clause 5.2.43).

use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry, geometry_type::GeometryType, query_coordinates::QueryCoordinates};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};
use thiserror::Error;

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

/// The two wire members that jointly describe an ordering geometry.
#[derive(Deserialize)]
struct OrderingGeometryWire {
    /// `GeoJSON` coordinates.
    ///
    /// Given directly in a Query body, and as JSON text in the GET `orderFrom`
    /// parameter; both bindings read the same coordinates.
    #[serde(alias = "orderFrom")]
    coordinates: QueryCoordinates,
    /// `GeoJSON` geometry type; clause 5.2.43 defaults it to `Point`.
    #[serde(alias = "orderGeometry", default = "default_geometry_type")]
    geometry: GeometryType,
}

/// The default geometry type required by Table 5.2.43-1.
const fn default_geometry_type() -> GeometryType {
    GeometryType::Point
}

/// A malformed clause-5.2.43 ordering geometry.
#[derive(Debug, Error)]
pub enum OrderingGeometryError {
    /// The geometry type and coordinates do not form a valid RFC 7946 geometry.
    ///
    /// The geometry diagnostic is carried verbatim, so an `orderFrom` position
    /// with an unusable ordinate count reaches the client naming that count.
    #[error("orderFrom reference geometry: {0}")]
    Geometry(#[from] GeometryError),
}

impl OrderingGeometry {
    /// Wrap a geometry into an ordering reference.
    #[must_use]
    pub const fn new(geometry: Geometry) -> OrderingGeometry {
        OrderingGeometry { geometry }
    }
}

// A derive cannot combine the separate `geometry` and `coordinates` members
// from Table 5.2.43-1 into the single validated `Geometry` invariant.
impl<'de> Deserialize<'de> for OrderingGeometry {
    fn deserialize<D>(deserializer: D) -> Result<OrderingGeometry, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = OrderingGeometryWire::deserialize(deserializer)?;
        let written = wire.coordinates.into_written().map_err(DeserializeError::custom)?;
        written
            .read_as(wire.geometry)
            .map(OrderingGeometry::new)
            .map_err(|error| DeserializeError::custom(OrderingGeometryError::Geometry(error)))
    }
}

// `Geometry` already serializes as the exact RFC 7946 object whose members
// Table 5.2.43-1 embeds, so forwarding its serializer avoids reconstructing
// coordinate arrays or cloning them.
impl Serialize for OrderingGeometry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.geometry.serialize(serializer)
    }
}
