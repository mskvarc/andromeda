//! A `GeoJSON` geometry written straight from its typed positions.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 makes the value of a `GeoProperty` an IETF
//! RFC 7946 geometry object, and the typed [`GeometryForm`] already holds it as
//! its type and positions. Writing those positions one ordinate at a time into
//! the response serializer is what keeps a geometry of a million positions
//! from being copied into any intermediate tree on its way to the wire. Each
//! ordinate is written as the `f64` it is held as, exactly as the `geojson`
//! rendering of the same geometry writes it.

use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use serde::{
    Serialize,
    Serializer,
    ser::{SerializeMap, SerializeSeq},
};

/// The member naming a `GeoJSON` geometry's type (IETF RFC 7946 section 3.1).
const TYPE_MEMBER: &str = "type";

/// The member holding a `GeoJSON` geometry's positions (IETF RFC 7946 section
/// 3.1).
const COORDINATES_MEMBER: &str = "coordinates";

/// One geometry object, written from its typed form.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GeometryBody<'a>(pub(crate) &'a GeometryForm);

impl Serialize for GeometryBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry(TYPE_MEMBER, &self.0.geometry_type())?;
        object.serialize_entry(COORDINATES_MEMBER, &Coordinates(self.0))?;
        object.end()
    }
}

/// The `coordinates` member of one geometry: nested arrays of positions whose
/// depth the geometry type decides (IETF RFC 7946 sections 3.1.2 to 3.1.7).
struct Coordinates<'a>(&'a GeometryForm);

impl Serialize for Coordinates<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            GeometryForm::Point(position) => PositionBody(position).serialize(serializer),
            GeometryForm::MultiPoint(positions) => Positions(positions).serialize(serializer),
            GeometryForm::LineString(line) => Positions(line.positions()).serialize(serializer),
            GeometryForm::MultiLineString(lines) => serializer.collect_seq(lines.iter().map(LineStringCoordinates::positions).map(Positions)),
            GeometryForm::Polygon(rings) => Rings(rings).serialize(serializer),
            GeometryForm::MultiPolygon(polygons) => serializer.collect_seq(polygons.iter().map(Rings)),
        }
    }
}

/// The rings of one Polygon, exterior ring first (IETF RFC 7946 section 3.1.6).
struct Rings<'a>(&'a PolygonRings);

impl Serialize for Rings<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.rings().iter().map(LinearRing::positions).map(Positions))
    }
}

/// An array of positions.
struct Positions<'a>(&'a [Position]);

impl Serialize for Positions<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(PositionBody))
    }
}

/// One position: longitude, latitude and, when it carries one, its altitude
/// (IETF RFC 7946 section 3.1.1).
struct PositionBody<'a>(&'a Position);

impl Serialize for PositionBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let altitude = self.0.altitude();
        let mut ordinates = serializer.serialize_seq(Some(if altitude.is_some() { 3 } else { 2 }))?;
        ordinates.serialize_element(&self.0.longitude())?;
        ordinates.serialize_element(&self.0.latitude())?;
        if let Some(altitude) = altitude {
            ordinates.serialize_element(&altitude)?;
        }
        ordinates.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_body::geometry_body::GeometryBody;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use serde_json::json;

    /// Every geometry type renders exactly as the `geojson` rendering of the
    /// same typed geometry does, altitude and ordinate spelling included.
    #[test]
    fn every_geometry_type_renders_as_its_geojson_rendering() {
        for written in [
            json!({"type": "Point", "coordinates": [13.4, 52.5]}),
            json!({"type": "Point", "coordinates": [13.0, 52.0, 34.5]}),
            json!({"type": "MultiPoint", "coordinates": [[1.0, 2.0], [3.5, 4.25]]}),
            json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0, 7.0]]}),
            json!({"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[2.0, 2.0], [3.0, 3.0]]]}),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]}),
            json!({"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]], [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]]]}),
        ] {
            let geometry = Geometry::from_geojson_value(&written).expect("a valid geometry");
            let streamed = serde_json::to_string(&GeometryBody(geometry.form())).expect("renders");
            let reference = serde_json::to_string(&geometry).expect("renders");
            pretty_assertions::assert_eq!(
                serde_json::from_str::<serde_json::Value>(&streamed).expect("JSON"),
                serde_json::from_str::<serde_json::Value>(&reference).expect("JSON"),
                "{written}"
            );
        }
    }
}
