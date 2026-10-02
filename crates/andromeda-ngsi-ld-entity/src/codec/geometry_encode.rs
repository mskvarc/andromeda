//! Writing a `GeoProperty` value back in its expanded form (clause 4.7).
//!
//! A geometry is written as the `GeoJSON` vocabulary node expansion produces:
//! `@type` naming the geometry type, `coordinates` as nested `@list`s of
//! `{"@value": n}` ordinates, then the `bbox` as one `@list` of ordinates and
//! each foreign member as the expanded values it holds. An ordinate is held as
//! a double, and is written as the canonical spelling of that double's
//! shortest round-trip decimal; the clause 4.7.2 string form is written back as
//! its text.

use crate::{
    codec::{
        geojson_members_codec::write_members,
        value_encode::{ListOut, TextOut},
    },
    geo_value::GeoValue,
    geometry_value::GeometryValue,
};
use andromeda_ngsi_ld_geometry::{
    bounding_box::BoundingBox,
    form::{GeometryForm, PolygonRings},
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use andromeda_ngsi_ld_number::float_spelling::canonical_float_text;
use andromeda_ngsi_ld_vocabulary::{expanded::properties, prefix};
use serde::{
    Serialize,
    Serializer,
    ser::{Error, SerializeMap},
};
use serde_json::Number;

/// One `GeoProperty` value, as the element of its `hasValue` array.
pub(crate) struct GeoValueOut<'a>(pub(crate) &'a GeoValue);

impl Serialize for GeoValueOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            GeoValue::Geometry(value) => GeometryOut(value).serialize(serializer),
            GeoValue::Encoded(encoded) => TextOut(encoded.as_str()).serialize(serializer),
        }
    }
}

/// A geometry as its `GeoJSON` vocabulary node.
struct GeometryOut<'a>(&'a GeometryValue);

impl Serialize for GeometryOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let form = self.0.form();
        let members = self.0.members();
        let type_iri = format!("{}{}", prefix::GEOJSON.as_str(), form.geometry_type());
        let entries = members.map_or(0, |members| usize::from(members.bbox().is_some()) + members.foreign().len());
        let mut map = serializer.serialize_map(Some(2 + entries))?;
        map.serialize_entry("@type", &[type_iri])?;
        map.serialize_entry(properties::COORDINATES.as_str(), &[CoordinatesOut(form)])?;
        if let Some(members) = members {
            write_members(&mut map, members)?;
        }
        map.end()
    }
}

/// A bounding box as one list of its ordinates.
pub(crate) struct BoundingBoxOut<'a>(pub(crate) &'a BoundingBox);

impl Serialize for BoundingBoxOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ListOut(BoxOrdinatesOut(self.0)).serialize(serializer)
    }
}

/// The ordinates of a bounding box, south-west corner first.
struct BoxOrdinatesOut<'a>(&'a BoundingBox);

impl Serialize for BoxOrdinatesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.ordinates().map(OrdinateOut))
    }
}

/// The root coordinate list of a geometry.
struct CoordinatesOut<'a>(&'a GeometryForm);

impl Serialize for CoordinatesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            GeometryForm::Point(position) => PositionOut(position).serialize(serializer),
            GeometryForm::MultiPoint(positions) => ListOut(MappedSeq(positions, PositionOut)).serialize(serializer),
            GeometryForm::LineString(line) => LineOut(line).serialize(serializer),
            GeometryForm::MultiLineString(lines) => ListOut(MappedSeq(lines, LineOut)).serialize(serializer),
            GeometryForm::Polygon(rings) => RingsOut(rings).serialize(serializer),
            GeometryForm::MultiPolygon(polygons) => ListOut(MappedSeq(polygons, RingsOut)).serialize(serializer),
        }
    }
}

/// The items of a slice, each written through its wrapper.
struct MappedSeq<'a, T, W>(&'a [T], fn(&'a T) -> W);

impl<T, W: Serialize> Serialize for MappedSeq<'_, T, W> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(self.1))
    }
}

/// One line.
struct LineOut<'a>(&'a LineStringCoordinates);

impl Serialize for LineOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ListOut(MappedSeq(self.0.positions(), PositionOut)).serialize(serializer)
    }
}

/// One Polygon, ring by ring.
struct RingsOut<'a>(&'a PolygonRings);

impl Serialize for RingsOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ListOut(MappedSeq(self.0.rings(), RingOut)).serialize(serializer)
    }
}

/// One ring.
struct RingOut<'a>(&'a LinearRing);

impl Serialize for RingOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ListOut(MappedSeq(self.0.positions(), PositionOut)).serialize(serializer)
    }
}

/// One position, two or three ordinates as written.
struct PositionOut<'a>(&'a Position);

impl Serialize for PositionOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ListOut(OrdinatesOut(self.0)).serialize(serializer)
    }
}

/// The ordinates of one position.
struct OrdinatesOut<'a>(&'a Position);

impl Serialize for OrdinatesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let position = self.0;
        let ordinates = [Some(position.longitude()), Some(position.latitude()), position.altitude()];
        serializer.collect_seq(ordinates.into_iter().flatten().map(OrdinateOut))
    }
}

/// One ordinate as `{"@value": n}`.
struct OrdinateOut(f64);

impl Serialize for OrdinateOut {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = canonical_float_text(self.0).map_err(S::Error::custom)?;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@value", &Number::from_string_unchecked(text))?;
        map.end()
    }
}
