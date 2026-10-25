//! The `coordinates` member of a geometry, written on its own.
//!
//! Table 5.2.13-1 of ETSI GS CIM 009 v1.9.1 carries a geo-query's geometry type
//! and coordinates as two sibling members, so the coordinates are written
//! apart from the geometry object they belong to, straight from the typed
//! positions and without any intermediate JSON tree.

use crate::{geometry::Geometry, geometry_writer::Coordinates};
use serde::{Serialize, Serializer};

/// The `coordinates` member of one geometry, as it serializes.
#[derive(Debug, Clone, Copy)]
pub struct CoordinatesMember<'a> {
    /// The geometry whose positions are written.
    geometry: &'a Geometry,
}

impl<'a> CoordinatesMember<'a> {
    /// The coordinates member of `geometry`.
    #[must_use]
    pub const fn new(geometry: &'a Geometry) -> CoordinatesMember<'a> {
        CoordinatesMember { geometry }
    }
}

// Hand-written: the member is the nested position arrays of the geometry's
// form alone, which no derive over the geometry expresses.
impl Serialize for CoordinatesMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Coordinates(self.geometry.form()).serialize(serializer)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{coordinates_member::CoordinatesMember, geometry::Geometry, geometry_type::GeometryType, query_coordinates::WrittenCoordinates};

    #[test]
    fn a_polygon_writes_only_its_nested_positions() {
        let text = "[[[0,0,12.5],[4,0,12.5],[4,4,12.5],[0,0,12.5]]]";
        let geometry: Geometry = WrittenCoordinates::from_text(text)
            .expect("valid JSON")
            .read_as(GeometryType::Polygon)
            .expect("a polygon");
        pretty_assertions::assert_eq!(serde_json::to_string(&CoordinatesMember::new(&geometry)).expect("serializes"), text);
    }
}
