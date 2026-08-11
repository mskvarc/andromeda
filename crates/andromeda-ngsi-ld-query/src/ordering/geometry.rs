//! The reference geometry a distance ordering measures from (clause 4.23, clause 5.2.43).

use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry, geometry_type::GeometryType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
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
    /// GeoJSON coordinates.
    #[serde(alias = "orderFrom", deserialize_with = "deserialize_coordinates")]
    coordinates: Value,
    /// GeoJSON geometry type; clause 5.2.43 defaults it to `Point`.
    #[serde(alias = "orderGeometry", default = "default_geometry_type")]
    geometry: GeometryType,
}

/// Accept JSON coordinates directly in a Query body and JSON text in the GET
/// `orderFrom` parameter, producing the same semantic value for both bindings.
fn deserialize_coordinates<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(raw) => serde_json::from_str(&raw).map_err(serde::de::Error::custom),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => Ok(value),
    }
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
        let value = serde_json::json!({
            "type": wire.geometry,
            "coordinates": wire.coordinates,
        });
        Geometry::from_geojson_value(&value)
            .map(OrderingGeometry::new)
            .map_err(|error| serde::de::Error::custom(OrderingGeometryError::Geometry(error)))
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
