//! A `GeoJSON` geometry written straight from its typed positions.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 makes the value of a `GeoProperty` an IETF
//! RFC 7946 geometry object, and a [`Geometry`] already holds it as its type
//! and positions, its `bbox` and its foreign members (sections 5 and 6.1).
//! Writing the positions one ordinate at a time into the serializer is what
//! keeps a geometry of a million positions from being copied into any
//! intermediate tree on its way to the wire. Each ordinate is written in the
//! canonical spelling of the double it is held as, the spelling the stored
//! document gives it (`13`, `0.0000001`); the `bbox` and the foreign members
//! follow the coordinates, as clause 4.6.4 returns the content of a value as
//! provided.

use crate::{
    bounding_box::BoundingBox,
    form::{GeometryForm, PolygonRings},
    geometry::Geometry,
    geometry_member::GeometryMember,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use andromeda_ngsi_ld_number::float_spelling::CanonicalFloat;
use serde::{
    Serialize,
    Serializer,
    ser::{SerializeMap, SerializeSeq},
};

// Hand-written: the geometry object is written member by member from the
// typed form, which no derive over the fields expresses.
impl Serialize for Geometry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let form = self.form();
        let members = self.members();
        let entries = members.map_or(0, |members| usize::from(members.bbox().is_some()) + members.foreign().iter().count());
        let mut object = serializer.serialize_map(Some(2 + entries))?;
        object.serialize_entry(GeometryMember::Type.as_str(), &form.geometry_type())?;
        object.serialize_entry(GeometryMember::Coordinates.as_str(), &Coordinates(form))?;
        if let Some(members) = members {
            if let Some(bbox) = members.bbox() {
                object.serialize_entry(GeometryMember::Bbox.as_str(), &BoundingBoxBody(bbox))?;
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
        object.serialize_entry(GeometryMember::Type.as_str(), &self.geometry_type())?;
        object.serialize_entry(GeometryMember::Coordinates.as_str(), &Coordinates(self))?;
        object.end()
    }
}

/// The numbers of a bounding box, south-west corner first (IETF RFC 7946
/// section 5).
struct BoundingBoxBody<'a>(&'a BoundingBox);

impl Serialize for BoundingBoxBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.ordinates().map(CanonicalFloat))
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
        ordinates.serialize_element(&CanonicalFloat(self.0.longitude()))?;
        ordinates.serialize_element(&CanonicalFloat(self.0.latitude()))?;
        if let Some(altitude) = altitude {
            ordinates.serialize_element(&CanonicalFloat(altitude))?;
        }
        ordinates.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::Geometry;
    use andromeda_ngsi_ld_number::float_spelling::canonical_float_text;

    /// IETF RFC 7946 sections 5 and 6.1, clause 4.5.16.2: the `bbox` and the
    /// foreign members render beside the coordinates, so a Feature's geometry
    /// is the one written.
    #[test]
    fn a_geometry_writes_its_box_and_foreign_members() {
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [1.5, 2], "bbox": [1.5, 2, 1.5, 2.0], "plotName": {"b": 1, "a": [2.5]}}"#),
            r#"{"type":"Point","coordinates":[1.5,2],"bbox":[1.5,2,1.5,2],"plotName":{"b":1,"a":[2.5]}}"#
        );
    }

    /// A bare form writes its type and positions.
    #[test]
    fn a_form_writes_its_type_and_positions() {
        let geometry = Geometry::from_geojson_text(r#"{"type": "Point", "coordinates": [1.5, 2], "bbox": [1.5, 2, 1.5, 2]}"#).expect("a valid geometry");
        pretty_assertions::assert_eq!(
            serde_json::to_string(geometry.form()).expect("renders"),
            r#"{"type":"Point","coordinates":[1.5,2]}"#
        );
    }

    /// The writer spells every ordinate the way the stored document does,
    /// with the one canonical conversion: a whole double without a fraction.
    #[test]
    fn an_ordinate_is_written_in_its_canonical_spelling() {
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [13, 52]}"#),
            r#"{"type":"Point","coordinates":[13,52]}"#
        );
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [-180.0, -0.0, 13.0]}"#),
            r#"{"type":"Point","coordinates":[-180,0,13]}"#
        );
    }

    /// An ordinate past the plain spellings takes the canonical conversion's
    /// rendering, exactly as the stored document writes it.
    #[test]
    fn an_ordinate_past_the_plain_spellings_is_written_as_the_canonical_conversion_renders_it() {
        for ordinate in [1e-7, 1.5e300, 0.001, 5e-324] {
            let canonical = canonical_float_text(ordinate).expect("a finite ordinate has a spelling");
            pretty_assertions::assert_eq!(
                written_back(&format!(r#"{{"type": "Point", "coordinates": [13.4, 52.5, {ordinate:e}]}}"#)),
                format!(r#"{{"type":"Point","coordinates":[13.4,52.5,{canonical}]}}"#),
                "{ordinate:e}"
            );
        }
    }

    /// A `bbox` is spelled as its corners' ordinates are.
    #[test]
    fn a_bounding_box_is_written_in_the_canonical_spelling() {
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [13, 52], "bbox": [13.0, 52.0, 13, 52]}"#),
            r#"{"type":"Point","coordinates":[13,52],"bbox":[13,52,13,52]}"#
        );
    }

    /// The text `geometry` is written back as after it is read from `written`.
    fn written_back(written: &str) -> String {
        serde_json::to_string(&Geometry::from_geojson_text(written).expect("a valid geometry")).expect("renders")
    }

    /// ETSI GS CIM 009 v1.9.1 clause 4.7.2 leaves the precision of a position to
    /// the implementation: an ordinate is held as the double nearest to it, so
    /// digits finer than the spacing of doubles are not kept.
    #[test]
    fn an_ordinate_with_more_digits_than_a_double_is_written_back_as_its_nearest_double() {
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [13.123456789012345678, 52.123456789012345678]}"#),
            r#"{"type":"Point","coordinates":[13.123456789012346,52.123456789012344]}"#
        );
    }

    #[test]
    fn an_altitude_with_more_digits_than_a_double_is_written_back_as_its_nearest_double() {
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [13.5, 52.5, 667.123456789012345678]}"#),
            r#"{"type":"Point","coordinates":[13.5,52.5,667.1234567890124]}"#
        );
    }

    #[test]
    fn an_ordinate_written_as_the_shortest_spelling_of_its_double_round_trips_byte_for_byte() {
        for written in [
            r#"{"type":"Point","coordinates":[179.8451356,-16.6429634]}"#,
            r#"{"type":"Point","coordinates":[13.123456789012346,52.5]}"#,
            r#"{"type":"Point","coordinates":[179.99999999999997,89.99999999999999]}"#,
            r#"{"type":"Point","coordinates":[-179.99999999999997,-89.99999999999999,-10994.25]}"#,
            r#"{"type":"LineString","coordinates":[[0.1,0.2],[14.55,46.05]]}"#,
        ] {
            pretty_assertions::assert_eq!(written_back(written), written);
        }
    }

    /// Spellings of one double read as that double and are written back in its
    /// shortest spelling.
    #[test]
    fn spellings_of_one_double_are_written_back_in_its_shortest_spelling() {
        for (written, expected) in [
            ("14.550000000000000001", "14.55"),
            ("0.10000000000000001", "0.1"),
            ("52.50", "52.5"),
            ("1.3123456789012345678e1", "13.123456789012346"),
        ] {
            pretty_assertions::assert_eq!(
                written_back(&format!(r#"{{"type": "Point", "coordinates": [{written}, 1.5]}}"#)),
                format!(r#"{{"type":"Point","coordinates":[{expected},1.5]}}"#),
                "{written}"
            );
        }
    }

    /// The CRS84 ranges of RFC 7946 section 4 are checked on the double an
    /// ordinate is held as, so an ordinate whose nearest double is an axis end
    /// is that axis end.
    #[test]
    fn an_ordinate_whose_nearest_double_is_an_axis_end_is_held_as_that_end() {
        pretty_assertions::assert_eq!(
            written_back(r#"{"type": "Point", "coordinates": [180.00000000000000001, -90.000000000000000001]}"#),
            r#"{"type":"Point","coordinates":[180,-90]}"#
        );
    }

    /// Every geometry type writes its nested coordinates in the canonical
    /// spelling, altitude included.
    #[test]
    fn every_geometry_type_writes_its_coordinates_in_the_canonical_spelling() {
        for written in [
            r#"{"type":"Point","coordinates":[13.4,52.5]}"#,
            r#"{"type":"Point","coordinates":[13,52,34.5]}"#,
            r#"{"type":"MultiPoint","coordinates":[[1,2],[3.5,4.25]]}"#,
            r#"{"type":"LineString","coordinates":[[0,0,3],[1,1,7]]}"#,
            r#"{"type":"MultiLineString","coordinates":[[[0,0],[1,1]],[[2,2],[3,3]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}"#,
            r#"{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]],[[[5,5],[6,5],[6,6],[5,5]]]]}"#,
        ] {
            pretty_assertions::assert_eq!(written_back(written), written);
        }
    }
}
