//! The `type` token of a `GeoJSON` geometry object, standing alone.
//!
//! ETSI GS CIM 009 v1.9.1 uses the bare token in places where no coordinates
//! accompany it, such as the `geometry` member of a GeoJSON-rendered query
//! result, so it is a value in its own right, distinct from
//! [`crate::form::GeometryForm`], which is the token together with the
//! coordinates it describes.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The `type` member of a `GeoJSON` geometry object (IETF RFC 7946 section 1.4).
///
/// The variant names are the RFC's tokens verbatim, which is also the casing
/// NGSI-LD carries them in (clause 4.7); no serde renaming applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum GeometryType {
    /// A single position.
    Point,
    /// An array of positions.
    MultiPoint,
    /// An array of two or more positions.
    LineString,
    /// An array of `LineString` coordinate arrays.
    MultiLineString,
    /// An array of linear ring coordinate arrays.
    Polygon,
    /// An array of Polygon coordinate arrays.
    MultiPolygon,
}

impl GeometryType {
    /// How many arrays the `coordinates` of a geometry of this type nest
    /// (IETF RFC 7946 section 3.1): one for a position, and one more for each
    /// array of positions around it.
    #[must_use]
    pub const fn coordinate_nesting(self) -> usize {
        match self {
            GeometryType::Point => 1,
            GeometryType::MultiPoint | GeometryType::LineString => 2,
            GeometryType::MultiLineString | GeometryType::Polygon => 3,
            GeometryType::MultiPolygon => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry_type::GeometryType;
    use serde_json::{Value, json};

    /// How many arrays `value` nests, following its first item.
    fn array_depth(value: &Value) -> usize {
        match value {
            Value::Array(items) => 1 + items.first().map_or(0, array_depth),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => 0,
        }
    }

    /// The nesting of each type is that of the RFC 7946 coordinates it carries.
    #[test]
    fn the_coordinate_nesting_is_the_depth_of_the_rfc_7946_coordinates() {
        for (geometry_type, coordinates) in [
            (GeometryType::Point, json!([1.0, 2.0])),
            (GeometryType::MultiPoint, json!([[1.0, 2.0]])),
            (GeometryType::LineString, json!([[1.0, 2.0], [3.0, 4.0]])),
            (GeometryType::MultiLineString, json!([[[1.0, 2.0], [3.0, 4.0]]])),
            (GeometryType::Polygon, json!([[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]])),
            (GeometryType::MultiPolygon, json!([[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]])),
        ] {
            pretty_assertions::assert_eq!(geometry_type.coordinate_nesting(), array_depth(&coordinates), "{geometry_type}");
        }
    }
}
