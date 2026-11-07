//! Reading a compact `GeoJSON` geometry object straight from a deserializer.
//!
//! A `GeoProperty` value of a million positions would cost a JSON tree of a
//! million arrays and two million number tokens before a single position was
//! read. The object is read member by member instead, each member into what it
//! says, and the geometry is built from them once the object ends, with the one
//! validation and the one set of refusals every geometry path shares.
//!
//! A value that is no geometry is still read to its end, so the refusal is the
//! [`GeometryError`] naming what is wrong rather than a decoding failure, and a
//! caller decoding a larger document goes on reading it.

use crate::{
    coordinate_tape::CoordinateTape,
    geojson::GeometryError,
    geometry::Geometry,
    geometry_census::GeometryPositionLimit,
    geometry_member_reading::{GeometryKey, GeometryKeySeed, read_member},
    geometry_members::{ForeignMemberReading, GeometryMembers, JsonLdKeywords},
};
use andromeda_ngsi_ld_number::json_value::value_census::MemberName;
use serde::de::{Deserialize, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
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
    /// Returns the [`GeometryError`] naming what the object lacks or misplaces,
    /// as [`Geometry::from_geojson_text`] gives for the same text.
    pub fn into_geometry(self) -> Result<Geometry, GeometryError> {
        self.geometry
    }

    /// Read a written geometry from `deserializer`, passing its foreign
    /// members over for a reader that wants the form alone.
    pub(crate) fn deserialize_without_foreign_members<'de, D: Deserializer<'de>>(deserializer: D) -> Result<WrittenGeometry, D::Error> {
        deserializer.deserialize_any(WrittenGeometryVisitor {
            foreign: ForeignMemberReading::Skipped,
            positions: None,
        })
    }

    /// Read a written geometry from the JSON text `text`, its `coordinates`
    /// recorded no further than a geometry of `limit` positions can hold.
    ///
    /// A geometry past the bound reads as a refused geometry, not a failure,
    /// and costs what the bound costs whatever the size of `text`.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure for text that is no JSON value.
    pub fn read_within(text: &str, limit: GeometryPositionLimit) -> Result<WrittenGeometry, serde_json::Error> {
        let mut reader = serde_json::Deserializer::from_str(text);
        let written = (&mut reader).deserialize_any(WrittenGeometryVisitor {
            foreign: ForeignMemberReading::Kept,
            positions: Some(limit),
        })?;
        reader.end()?;
        Ok(written)
    }

    /// The position limit the object's `coordinates` held more ordinates than,
    /// when they did.
    #[must_use]
    pub const fn exceeded_positions(&self) -> Option<GeometryPositionLimit> {
        match &self.geometry {
            Err(GeometryError::PositionLimit { limit }) => Some(*limit),
            Ok(_)
            | Err(
                GeometryError::Position { .. }
                | GeometryError::LineString { .. }
                | GeometryError::LinearRing { .. }
                | GeometryError::MixedArity { .. }
                | GeometryError::NotAnObject
                | GeometryError::EncodedNotJson(_)
                | GeometryError::MissingType
                | GeometryError::UnknownType(_)
                | GeometryError::MissingCoordinates
                | GeometryError::MissingGeometries
                | GeometryError::InvalidBbox(_)
                | GeometryError::NotAnArray { .. }
                | GeometryError::GeometryCollectionUnsupported
                | GeometryError::ForeignMember(_),
            ) => None,
        }
    }

    /// Whether a member of the object is named by a JSON-LD keyword.
    #[must_use]
    pub const fn json_ld_keywords(&self) -> JsonLdKeywords {
        self.json_ld_keywords
    }

    /// The written geometry `members` spell.
    fn of_members(members: GeometryMembers, positions: Option<GeometryPositionLimit>) -> WrittenGeometry {
        let json_ld_keywords = members.json_ld_keywords;
        let geometry = match positions {
            Some(limit) if members.coordinates.as_ref().is_some_and(CoordinateTape::overflowed) => Err(GeometryError::PositionLimit { limit }),
            Some(_) | None => members.into_geometry(),
        };
        WrittenGeometry { geometry, json_ld_keywords }
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
        deserializer.deserialize_any(WrittenGeometryVisitor {
            foreign: ForeignMemberReading::Kept,
            positions: None,
        })
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
struct WrittenGeometryVisitor {
    /// Whether the foreign members the object carries are kept.
    foreign: ForeignMemberReading,
    /// The positions a geometry may hold, when `coordinates` is recorded only
    /// that far.
    positions: Option<GeometryPositionLimit>,
}

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
            return Ok(WrittenGeometry::of_members(members, self.positions));
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
            read_member(&mut map, key, self.foreign, self.positions, &mut members)?;
            match map.next_key_seed(GeometryKeySeed)? {
                Some(next) => key = next,
                None => return Ok(WrittenGeometry::of_members(members, self.positions)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        geojson::GeometryError,
        geometry::Geometry,
        geometry_census::GeometryPositionLimit,
        geometry_members::JsonLdKeywords,
        written_geometry::WrittenGeometry,
    };
    use serde_json::Value;
    use std::num::NonZeroUsize;

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

    /// The reading over a parsed tree agrees with the reading of its text, in
    /// what it accepts, its box and foreign members included, in what it
    /// refuses and why.
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

    fn limit(positions: usize) -> GeometryPositionLimit {
        GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit"))
    }

    /// A geometry holding no more ordinates than the limit allows reads as it
    /// does unbounded.
    #[test]
    fn a_geometry_within_the_position_limit_reads_as_it_does_without_one() {
        let text = r#"{"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}"#;
        let written = WrittenGeometry::read_within(text, limit(5)).expect("read");
        assert!(written.exceeded_positions().is_none());
        pretty_assertions::assert_eq!(written.into_geometry().expect("a polygon").form().position_count().get(), 5);
    }

    /// A geometry with more ordinates than three per permitted position reads
    /// as past the limit, whatever its type or the order of its members, and
    /// the reading goes on to the end of the text.
    #[test]
    fn a_geometry_past_the_position_limit_reads_as_exceeding_it() {
        for text in [
            r#"{"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2], [3, 3]]}"#,
            r#"{"coordinates": [[0, 0], [1, 1], [2, 2], [3, 3]], "type": "LineString", "foreign": 1}"#,
        ] {
            let written = WrittenGeometry::read_within(text, limit(1)).expect("read to its end");
            pretty_assertions::assert_eq!(written.exceeded_positions(), Some(limit(1)), "{text}");
            assert!(matches!(written.into_geometry(), Err(GeometryError::PositionLimit { .. })), "{text}");
        }
    }

    /// A text that is no JSON is the reader's failure, not an oversized
    /// geometry.
    #[test]
    fn a_text_that_is_no_json_is_a_reading_failure_under_the_limit() {
        assert!(WrittenGeometry::read_within(r#"{"type": "Point", "coordinates": [1, "#, limit(1)).is_err());
    }

    impl WrittenGeometry {
        /// The written geometry read from a parsed tree through serde.
        fn deserialize_from(tree: &Value) -> WrittenGeometry {
            serde::Deserialize::deserialize(tree).expect("a tree is read to its end")
        }
    }
}
