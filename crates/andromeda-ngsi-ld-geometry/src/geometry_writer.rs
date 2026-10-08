//! A `GeoJSON` geometry written straight from its typed positions.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 makes the value of a `GeoProperty` an IETF
//! RFC 7946 geometry object, and a [`Geometry`] already holds it as its type
//! and positions, its `bbox` and its foreign members (sections 5 and 6.1).
//! Writing the positions one ordinate at a time into the serializer is what
//! keeps a geometry of a million positions from being copied into any
//! intermediate tree on its way to the wire. Each ordinate is written as the
//! `f64` it is held as; the `bbox` and the foreign members follow the
//! coordinates, as clause 4.6.4 returns the content of a value as provided.

use crate::{
    bounding_box::BoundingBox,
    form::{GeometryForm, PolygonRings},
    geometry::Geometry,
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

/// The member holding a `GeoJSON` geometry's bounding box (IETF RFC 7946
/// section 5).
const BBOX_MEMBER: &str = "bbox";

// Hand-written: the geometry object is written member by member from the
// typed form, which no derive over the fields expresses.
impl Serialize for Geometry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let form = self.form();
        let members = self.members();
        let entries = members.map_or(0, |members| usize::from(members.bbox().is_some()) + members.foreign().iter().count());
        let mut object = serializer.serialize_map(Some(2 + entries))?;
        object.serialize_entry(TYPE_MEMBER, &form.geometry_type())?;
        object.serialize_entry(COORDINATES_MEMBER, &Coordinates(form))?;
        if let Some(members) = members {
            if let Some(bbox) = members.bbox() {
                object.serialize_entry(BBOX_MEMBER, &BoundingBoxBody(bbox))?;
            }
            for (name, value) in members.foreign().iter() {
                object.serialize_entry(name.as_ref(), value)?;
            }
        }
        object.end()
    }
}

// Hand-written for the same reason: a bare form is the geometry object of its
// type and positions alone.
impl Serialize for GeometryForm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry(TYPE_MEMBER, &self.geometry_type())?;
        object.serialize_entry(COORDINATES_MEMBER, &Coordinates(self))?;
        object.end()
    }
}

/// The numbers of a bounding box, south-west corner first (IETF RFC 7946
/// section 5).
struct BoundingBoxBody<'a>(&'a BoundingBox);

impl Serialize for BoundingBoxBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.ordinates())
    }
}

/// The `coordinates` member of one geometry: nested arrays of positions whose
/// depth the geometry type decides (IETF RFC 7946 sections 3.1.2 to 3.1.7).
pub(crate) struct Coordinates<'a>(pub(crate) &'a GeometryForm);

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
    use crate::geometry::Geometry;
    use serde_json::json;

    /// IETF RFC 7946 sections 5 and 6.1, clause 4.5.16.2: the `bbox` and the
    /// foreign members render beside the coordinates, so a Feature's geometry
    /// is the one written.
    #[test]
    fn a_geometry_writes_its_box_and_foreign_members() {
        let written = json!({"type": "Point", "coordinates": [1.0, 2.0], "bbox": [1.0, 2.0, 1.0, 2.0], "plotName": {"b": 1, "a": [2.5]}});
        let geometry = Geometry::from_geojson_text(&written.to_string()).expect("a valid geometry");
        let streamed: serde_json::Value = serde_json::from_str(&serde_json::to_string(&geometry).expect("renders")).expect("JSON");
        pretty_assertions::assert_eq!(streamed, written);
    }

    /// A bare form writes its type and positions.
    #[test]
    fn a_form_writes_its_type_and_positions() {
        let written = json!({"type": "Point", "coordinates": [1.0, 2.0], "bbox": [1.0, 2.0, 1.0, 2.0]});
        let geometry = Geometry::from_geojson_text(&written.to_string()).expect("a valid geometry");
        let streamed: serde_json::Value = serde_json::from_str(&serde_json::to_string(geometry.form()).expect("renders")).expect("JSON");
        pretty_assertions::assert_eq!(streamed, json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    /// Every geometry type renders as the `geojson` rendering of the same
    /// positions does, altitude and ordinate spelling included.
    #[test]
    fn every_geometry_type_renders_as_its_geojson_rendering() {
        for written in [
            json!({"type": "Point", "coordinates": [13.4, 52.5]}),
            json!({"type": "Point", "coordinates": [13.0, 52.0, 34.5]}),
            json!({"type": "MultiPoint", "coordinates": [[1.0, 2.0], [3.5, 4.25]]}),
            json!({"type": "LineString", "coordinates": [[0.0, 0.0, 3.0], [1.0, 1.0, 7.0]]}),
            json!({"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[2.0, 2.0], [3.0, 3.0]]]}),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]}),
            json!({"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]], [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]]]}),
        ] {
            let geometry = Geometry::from_geojson_text(&written.to_string()).expect("a valid geometry");
            let streamed = serde_json::to_string(&geometry).expect("renders");
            let reference = serde_json::to_string(&geometry.form().to_geojson()).expect("renders");
            pretty_assertions::assert_eq!(
                serde_json::from_str::<serde_json::Value>(&streamed).expect("JSON"),
                serde_json::from_str::<serde_json::Value>(&reference).expect("JSON"),
                "{written}"
            );
        }
    }
}
