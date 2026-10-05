//! Reading a compact `GeoJSON` geometry object straight from a deserializer.
//!
//! A `GeoProperty` value of a million positions would cost a JSON tree of a
//! million arrays and two million number tokens before a single position was
//! read. The object is read member by member instead: `coordinates` and `bbox`
//! onto a [`CoordinateTape`] each, a foreign member (IETF RFC 7946 section 6.1)
//! into the JSON value it holds, every other member into what it says, and the
//! geometry is built from them once the object ends, with exactly the
//! validation and refusals [`Geometry::from_geojson_value`] applies to a parsed
//! tree.
//!
//! A value that is no geometry is still read to its end, so the refusal is the
//! [`GeometryError`] naming what is wrong rather than a decoding failure, and a
//! caller decoding a larger document goes on reading it.

use crate::{
    coordinate_tape::{CoordinateTape, CoordinateTapeSeed},
    foreign_members::ForeignMemberName,
    geojson::GeometryError,
    geometry::Geometry,
    geometry_members::{GeometriesMember, GeometryMembers, JsonLdKeywords, TypeMember},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_number::json_value::value_census::{MemberName, MemberNameSeed};
use serde::de::{Deserialize, DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// A compact `GeoJSON` geometry object as written: the geometry it reads as,
/// or why it reads as none, and whether it carries a JSON-LD keyword member.
#[derive(Debug)]
pub struct WrittenGeometry {
    /// The geometry the object spells, or why it spells none.
    geometry: Result<Geometry, GeometryError>,
    /// Whether a member of the object is named by a JSON-LD keyword.
    json_ld_keywords: JsonLdKeywords,
}

impl WrittenGeometry {
    /// The geometry the object spells, with its `bbox` and its foreign
    /// members.
    ///
    /// # Errors
    ///
    /// Returns the [`GeometryError`] [`Geometry::from_geojson_value`] gives for
    /// the same object.
    pub fn into_geometry(self) -> Result<Geometry, GeometryError> {
        self.geometry
    }

    /// Whether a member of the object is named by a JSON-LD keyword.
    #[must_use]
    pub const fn json_ld_keywords(&self) -> JsonLdKeywords {
        self.json_ld_keywords
    }

    /// The written geometry `members` spell.
    fn of_members(members: GeometryMembers) -> WrittenGeometry {
        let json_ld_keywords = members.json_ld_keywords;
        WrittenGeometry {
            geometry: members.into_geometry(),
            json_ld_keywords,
        }
    }

    /// A written value that is no JSON object.
    const fn not_an_object() -> WrittenGeometry {
        WrittenGeometry {
            geometry: Err(GeometryError::NotAnObject),
            json_ld_keywords: JsonLdKeywords::Absent,
        }
    }
}

// Hand-written: the members are read into what they say rather than into a
// tree, and a value that is no geometry is a successful reading of a refused
// geometry, neither of which a derive expresses.
impl<'de> Deserialize<'de> for WrittenGeometry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WrittenGeometry, D::Error> {
        deserializer.deserialize_any(WrittenGeometryVisitor)
    }
}

// Hand-written: decoded through the written geometry, without a JSON tree; a
// refused geometry fails the decoding with its `GeometryError`.
impl<'de> Deserialize<'de> for Geometry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Geometry, D::Error> {
        WrittenGeometry::deserialize(deserializer)?.into_geometry().map_err(DeserializeError::custom)
    }
}

/// Reads one written geometry.
#[derive(Clone, Copy, Debug)]
struct WrittenGeometryVisitor;

impl<'de> Visitor<'de> for WrittenGeometryVisitor {
    type Value = WrittenGeometry;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a GeoJSON geometry object")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_i128<E: DeserializeError>(self, _value: i128) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_u128<E: DeserializeError>(self, _value: u128) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_none<E: DeserializeError>(self) -> Result<WrittenGeometry, E> {
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<WrittenGeometry, A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        Ok(WrittenGeometry::not_an_object())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<WrittenGeometry, A::Error> {
        let mut members = GeometryMembers::default();
        let Some(first) = map.next_key_seed(GeometryKeySeed)? else {
            return Ok(WrittenGeometry::of_members(members));
        };
        // The reader's spelling of a number, which is no object.
        if let GeometryKey::Other(name) = &first
            && MemberName::of(name) == MemberName::NumberToken
        {
            map.next_value::<IgnoredAny>()?;
            return Ok(WrittenGeometry::not_an_object());
        }
        let mut key = first;
        loop {
            read_member(&mut map, key, &mut members)?;
            match map.next_key_seed(GeometryKeySeed)? {
                Some(next) => key = next,
                None => return Ok(WrittenGeometry::of_members(members)),
            }
        }
    }
}

/// Read the value of the member `key` into `members`.
fn read_member<'de, A: MapAccess<'de>>(map: &mut A, key: GeometryKey, members: &mut GeometryMembers) -> Result<(), A::Error> {
    match key {
        GeometryKey::Type => {
            members.geometry_type = match map.next_value_seed(MemberShapeSeed)? {
                MemberShape::Text(token) => TypeMember::of(&token),
                MemberShape::Number | MemberShape::Array | MemberShape::Other => TypeMember::Missing,
            };
        }
        GeometryKey::Coordinates => members.coordinates = Some(read_tape(map)?),
        GeometryKey::Bbox => members.bbox = Some(read_tape(map)?),
        GeometryKey::Geometries => {
            members.geometries = match map.next_value_seed(MemberShapeSeed)? {
                MemberShape::Array => GeometriesMember::Array,
                MemberShape::Text(_) | MemberShape::Number | MemberShape::Other => GeometriesMember::Absent,
            };
        }
        GeometryKey::Other(name) => {
            if let Ok(foreign) = ForeignMemberName::try_from(&*name) {
                members.foreign.insert(foreign, map.next_value::<JsonLiteral>()?);
            } else {
                members.json_ld_keywords = members.json_ld_keywords.and(JsonLdKeywords::of(&name));
                map.next_value::<IgnoredAny>()?;
            }
        }
    }
    Ok(())
}

/// Record the value of the current member onto a tape.
fn read_tape<'de, A: MapAccess<'de>>(map: &mut A) -> Result<CoordinateTape, A::Error> {
    let mut tape = CoordinateTape::default();
    map.next_value_seed(CoordinateTapeSeed::new(&mut tape))?;
    Ok(tape)
}

/// The member of a geometry object a member name names.
#[derive(Debug)]
enum GeometryKey {
    /// `type`.
    Type,
    /// `coordinates`.
    Coordinates,
    /// `bbox`.
    Bbox,
    /// `geometries`.
    Geometries,
    /// Any other member, by name.
    Other(Box<str>),
}

/// Reads one member name of a geometry object.
#[derive(Clone, Copy, Debug)]
struct GeometryKeySeed;

impl<'de> DeserializeSeed<'de> for GeometryKeySeed {
    type Value = GeometryKey;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<GeometryKey, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for GeometryKeySeed {
    type Value = GeometryKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<GeometryKey, E> {
        Ok(match name {
            "type" => GeometryKey::Type,
            "coordinates" => GeometryKey::Coordinates,
            "bbox" => GeometryKey::Bbox,
            "geometries" => GeometryKey::Geometries,
            other => GeometryKey::Other(Box::from(other)),
        })
    }
}

/// What a member value is, as far as a geometry object's `type` and
/// `geometries` ask.
#[derive(Debug)]
enum MemberShape {
    /// A string, and its text.
    Text(String),
    /// A number.
    Number,
    /// An array.
    Array,
    /// A boolean, null, or an object.
    Other,
}

/// Reads one member value into its [`MemberShape`], to its end.
#[derive(Clone, Copy, Debug)]
struct MemberShapeSeed;

impl<'de> DeserializeSeed<'de> for MemberShapeSeed {
    type Value = MemberShape;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<MemberShape, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for MemberShapeSeed {
    type Value = MemberShape;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<MemberShape, E> {
        Ok(MemberShape::Other)
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_i128<E: DeserializeError>(self, _value: i128) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_u128<E: DeserializeError>(self, _value: u128) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_str<E: DeserializeError>(self, value: &str) -> Result<MemberShape, E> {
        Ok(MemberShape::Text(value.to_owned()))
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<MemberShape, E> {
        Ok(MemberShape::Other)
    }

    fn visit_none<E: DeserializeError>(self) -> Result<MemberShape, E> {
        Ok(MemberShape::Other)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<MemberShape, A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        Ok(MemberShape::Array)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<MemberShape, A::Error> {
        match members.next_key_seed(MemberNameSeed)? {
            // The reader's spelling of a number that is not a 64-bit integer.
            Some(MemberName::NumberToken) => {
                members.next_value::<IgnoredAny>()?;
                Ok(MemberShape::Number)
            }
            Some(MemberName::Member) => {
                members.next_value::<IgnoredAny>()?;
                while members.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(MemberShape::Other)
            }
            None => Ok(MemberShape::Other),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{geojson::GeometryError, geometry::Geometry, geometry_members::JsonLdKeywords, written_geometry::WrittenGeometry};
    use serde_json::Value;

    /// Geometry texts covering every refusal and acceptance, written with
    /// their members in more than one order.
    const TEXTS: [&str; 33] = [
        r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#,
        r#"{"coordinates": [1.0, 2.0, 3.5], "type": "Point"}"#,
        r#"{"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}"#,
        r#"{"type": "Point", "coordinates": [1.0]}"#,
        r#"{"type": "Point", "coordinates": [181.0, 0.0]}"#,
        r#"{"type": "Point", "coordinates": [1e400, 0.0]}"#,
        r#"{"type": "Point", "coordinates": [14.550000000000000001, 46.05]}"#,
        r#"{"type": "MultiPoint", "coordinates": [[1.0, 2.0], [3.0, 4.0, 5.0]]}"#,
        r#"{"type": "MultiPoint", "coordinates": [[0.0, 0.0], "secret"]}"#,
        r#"{"type": "LineString", "coordinates": [[1.0, 2.0]]}"#,
        r#"{"coordinates": [[0, 0], [1, 1]], "type": "LineString", "bbox": [0, 0, 1, 1]}"#,
        r#"{"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], []]}"#,
        r#"{"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]]}"#,
        r#"{"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]]}"#,
        r#"{"type": "Polygon", "coordinates": []}"#,
        r#"{"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]], [[[5.0, 5.0], [6.0, 5.0], [5.0, 5.0]]]]}"#,
        r#"{"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]], "bbox": [0.0, 0.0, 1.0, 1.0], "foreign": {"x": [1, 2]}}"#,
        r#"{"type": "Point", "coordinates": [1.0, 2.0], "bbox": "box"}"#,
        r#"{"type": "Point", "coordinates": [1.0, 2.0], "bbox": [1, "x"]}"#,
        r#"{"type": "Point", "coordinates": [1, 2], "bbox": [1, 2, 1, 2]}"#,
        r#"{"type": "Point", "coordinates": [1, 2], "bbox": [1, 2, 3]}"#,
        r#"{"type": "Point", "coordinates": [1, 2], "bbox": [1, 2, 1, 2], "title": "x", "nested": {"a": [1, 2]}}"#,
        r#"{"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]}"#,
        r#"{"type": "GeometryCollection"}"#,
        r#"{"type": "Nonsense", "coordinates": []}"#,
        r#"{"type": 7, "coordinates": [1, 2]}"#,
        r#"{"coordinates": [1, 2]}"#,
        r#"{"type": "Point"}"#,
        r#"{"type": "Point", "coordinates": "1,2"}"#,
        r#"{"type": "Point", "coordinates": [1, 2], "type": "LineString"}"#,
        r#""text""#,
        "[1, 2]",
        "{}",
    ];

    /// What a reading gave, rendered for comparison.
    fn outcome(geometry: Result<Geometry, GeometryError>) -> Result<Geometry, String> {
        geometry.map_err(|refusal| refusal.to_string())
    }

    /// The streamed reading of a text agrees with the reading of its tree,
    /// in what it accepts, its box and foreign members included, in what it
    /// refuses and why.
    #[test]
    fn a_streamed_geometry_reads_as_its_parsed_tree_does() {
        for text in TEXTS {
            let tree: Value = serde_json::from_str(text).expect("valid JSON");
            let streamed = serde_json::from_str::<WrittenGeometry>(text).expect("every value is read to its end");
            pretty_assertions::assert_eq!(outcome(streamed.into_geometry()), outcome(Geometry::from_geojson_value(&tree)), "{text}");
        }
    }

    /// The reading over a parsed tree agrees with the reading of its text.
    #[test]
    fn a_geometry_read_from_a_tree_reads_as_its_text_does() {
        for text in TEXTS {
            let tree: Value = serde_json::from_str(text).expect("valid JSON");
            let from_tree = WrittenGeometry::deserialize_from(&tree);
            let from_text = serde_json::from_str::<WrittenGeometry>(text).expect("every value is read to its end");
            pretty_assertions::assert_eq!(outcome(from_tree.into_geometry()), outcome(from_text.into_geometry()), "{text}");
        }
    }

    /// IETF RFC 7946 sections 5 and 6.1: the box and the foreign members are
    /// kept.
    #[test]
    fn a_written_geometry_keeps_its_box_and_foreign_members() {
        let geometry = serde_json::from_str::<WrittenGeometry>(r#"{"title": "x", "type": "Point", "coordinates": [1, 2], "bbox": [1, 2, 1, 2], "z": 1}"#)
            .expect("read")
            .into_geometry()
            .expect("a geometry");
        let members = geometry.members().expect("members are kept");
        pretty_assertions::assert_eq!(members.bbox().map(|bbox| bbox.ordinates().collect::<Vec<_>>()), Some(vec![1.0, 2.0, 1.0, 2.0]));
        let foreign: Vec<(String, &str)> = members.foreign().iter().map(|(name, value)| (name.to_string(), value.text())).collect();
        pretty_assertions::assert_eq!(foreign, vec![("title".to_owned(), r#""x""#), ("z".to_owned(), "1")]);
    }

    #[test]
    fn a_member_named_by_a_json_ld_keyword_is_reported() {
        let written = serde_json::from_str::<WrittenGeometry>(r#"{"@context": {}, "type": "Point", "coordinates": [13.4, 52.5]}"#).expect("read");
        pretty_assertions::assert_eq!(written.json_ld_keywords(), JsonLdKeywords::Present);
        let geometry = written.into_geometry().expect("the geometry itself is read");
        assert!(geometry.members().is_none(), "a keyword is never kept as a foreign member");
        let written = serde_json::from_str::<WrittenGeometry>(r#"{"type": "Point", "coordinates": [13.4, 52.5], "title": "x"}"#).expect("read");
        pretty_assertions::assert_eq!(written.json_ld_keywords(), JsonLdKeywords::Absent);
    }

    /// A refused geometry fails the decoding of a `Geometry` with the
    /// refusal's own words.
    #[test]
    fn deserialising_a_refused_geometry_fails_with_its_geometry_error() {
        let error = serde_json::from_str::<Geometry>(r#"{"type": "Point", "coordinates": [0.0, 91.0]}"#).expect_err("latitude 91");
        assert!(error.to_string().contains("latitude 91 is outside [-90, 90]"), "{error}");
        let error = serde_json::from_str::<Geometry>(r#"{"type": "Point", "coordinates": [0, 0], "bbox": [0, 0, 4]}"#).expect_err("three numbers");
        assert!(error.to_string().contains("RFC 7946 section 5"), "{error}");
    }

    /// A value that is no geometry is read to its end, so a document holding
    /// it goes on being read.
    #[test]
    fn a_value_that_is_no_geometry_is_read_to_its_end() {
        let read: Vec<WrittenGeometry> = serde_json::from_str(r#"[[1, [2]], {"a": {"b": 1}}, 7, {"type": "Point", "coordinates": [1, 2]}]"#).expect("read");
        let forms: Vec<bool> = read.into_iter().map(|written| written.into_geometry().is_ok()).collect();
        pretty_assertions::assert_eq!(forms, vec![false, false, false, true]);
    }

    impl WrittenGeometry {
        /// The written geometry read from a parsed tree through serde.
        fn deserialize_from(tree: &Value) -> WrittenGeometry {
            serde::Deserialize::deserialize(tree).expect("a tree is read to its end")
        }
    }
}
