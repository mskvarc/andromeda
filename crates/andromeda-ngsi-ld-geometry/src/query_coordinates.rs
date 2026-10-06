//! Coordinates given beside a separate geometry type, read without a tree.
//!
//! NGSI-LD query parameters and `geoQ` objects (ETSI GS CIM 009 v1.9.1 clause
//! 4.10, Table 5.2.13-1) name a geometry type and carry its `coordinates` as a
//! sibling member, either as JSON or "encoded as a string". Both spellings are
//! recorded straight from the deserializer, or from the text, onto the flat
//! coordinate tape and read as the type's nesting afterwards, so the number
//! of values never becomes a tree.

use crate::{
    coordinate_tape::{CoordinateTape, CoordinateTapeSeed},
    geojson::GeometryError,
    geometry::Geometry,
    geometry_type::GeometryType,
};
use serde::de::{Deserialize, DeserializeSeed, Deserializer, Error as DeserializeError, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// Coordinates recorded as written, not yet read as a geometry type's nesting.
#[derive(Debug)]
pub struct WrittenCoordinates {
    /// The coordinates, in document order.
    tape: CoordinateTape,
}

impl WrittenCoordinates {
    /// Record coordinates written as JSON text.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` failure for text that is no single JSON value.
    pub fn from_text(text: &str) -> Result<WrittenCoordinates, serde_json::Error> {
        CoordinateTape::of_text(text).map(|tape| WrittenCoordinates { tape })
    }

    /// Read the coordinates as the nesting `geometry_type` requires (IETF RFC
    /// 7946 sections 3.1.2 to 3.1.7).
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NotAnArray`] or
    /// [`GeometryError::NonNumericOrdinate`] for a member that breaks the
    /// nesting, [`GeometryError::Position`] for a position RFC 7946 gives no
    /// meaning, [`GeometryError::LineString`] for a line of fewer than two
    /// positions and [`GeometryError::LinearRing`] for a ring that is open or
    /// holds fewer than four.
    pub fn read_as(&self, geometry_type: GeometryType) -> Result<Geometry, GeometryError> {
        self.tape.read_as(geometry_type).map(Geometry::new)
    }
}

// Hand-written: the coordinates are recorded by a seed as the deserializer
// produces them, which no derive over a field expresses.
impl<'de> Deserialize<'de> for WrittenCoordinates {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WrittenCoordinates, D::Error> {
        let mut tape = CoordinateTape::default();
        CoordinateTapeSeed::new(&mut tape).deserialize(deserializer)?;
        Ok(WrittenCoordinates { tape })
    }
}

/// The `coordinates` of a query: JSON, or JSON text held in a string.
#[derive(Debug)]
pub enum QueryCoordinates {
    /// Coordinates encoded as a JSON string (Table 5.2.13-1), kept as written.
    Text(String),
    /// Coordinates given as JSON, recorded as written.
    Written(WrittenCoordinates),
}

impl QueryCoordinates {
    /// The recorded coordinates, the text of an encoded string recorded now.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` failure for an encoded string that is no
    /// single JSON value.
    pub fn into_written(self) -> Result<WrittenCoordinates, serde_json::Error> {
        match self {
            QueryCoordinates::Text(text) => WrittenCoordinates::from_text(&text),
            QueryCoordinates::Written(written) => Ok(written),
        }
    }
}

// Hand-written: a string stays text while every other JSON shape is recorded
// onto the tape as it is read, which no derive over an enum expresses.
impl<'de> Deserialize<'de> for QueryCoordinates {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<QueryCoordinates, D::Error> {
        deserializer.deserialize_any(QueryCoordinatesVisitor)
    }
}

/// Reads one `coordinates` value, text kept as text and the rest recorded.
struct QueryCoordinatesVisitor;

/// Record one scalar by the tape seed and return it as written coordinates.
macro_rules! record_scalar {
    ($($method:ident($value:ty)),* $(,)?) => {
        $(
            fn $method<E: DeserializeError>(self, value: $value) -> Result<QueryCoordinates, E> {
                let mut tape = CoordinateTape::default();
                CoordinateTapeSeed::new(&mut tape).$method(value)?;
                Ok(QueryCoordinates::Written(WrittenCoordinates { tape }))
            }
        )*
    };
}

impl<'de> Visitor<'de> for QueryCoordinatesVisitor {
    type Value = QueryCoordinates;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeoJSON coordinates, or JSON text encoding them")
    }

    record_scalar!(
        visit_bool(bool),
        visit_i64(i64),
        visit_u64(u64),
        visit_i128(i128),
        visit_u128(u128),
        visit_f64(f64)
    );

    fn visit_str<E: DeserializeError>(self, text: &str) -> Result<QueryCoordinates, E> {
        Ok(QueryCoordinates::Text(text.to_owned()))
    }

    fn visit_string<E: DeserializeError>(self, text: String) -> Result<QueryCoordinates, E> {
        Ok(QueryCoordinates::Text(text))
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<QueryCoordinates, E> {
        let mut tape = CoordinateTape::default();
        CoordinateTapeSeed::new(&mut tape).visit_unit::<E>()?;
        Ok(QueryCoordinates::Written(WrittenCoordinates { tape }))
    }

    fn visit_none<E: DeserializeError>(self) -> Result<QueryCoordinates, E> {
        self.visit_unit()
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<QueryCoordinates, A::Error> {
        let mut tape = CoordinateTape::default();
        CoordinateTapeSeed::new(&mut tape).visit_seq(items)?;
        Ok(QueryCoordinates::Written(WrittenCoordinates { tape }))
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<QueryCoordinates, A::Error> {
        let mut tape = CoordinateTape::default();
        CoordinateTapeSeed::new(&mut tape).visit_map(members)?;
        Ok(QueryCoordinates::Written(WrittenCoordinates { tape }))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        geojson::GeometryError,
        geometry_type::GeometryType,
        position::PositionError,
        query_coordinates::{QueryCoordinates, WrittenCoordinates},
    };

    #[test]
    fn text_and_a_json_array_read_as_the_same_point() {
        let from_text = WrittenCoordinates::from_text("[1.5, 2.5]")
            .expect("valid JSON")
            .read_as(GeometryType::Point)
            .expect("a point");
        let written: QueryCoordinates = serde_json::from_str("[1.5, 2.5]").expect("valid JSON");
        let QueryCoordinates::Written(written) = written else {
            panic!("an array is recorded, not kept as text");
        };
        pretty_assertions::assert_eq!(written.read_as(GeometryType::Point).expect("a point"), from_text);
    }

    #[test]
    fn a_json_string_stays_text_until_it_is_recorded() {
        let coordinates: QueryCoordinates = serde_json::from_str(r#""[1.5, 2.5]""#).expect("valid JSON");
        let QueryCoordinates::Text(text) = &coordinates else {
            panic!("a string is kept as text");
        };
        pretty_assertions::assert_eq!(text, "[1.5, 2.5]");
        assert!(coordinates.into_written().expect("valid text").read_as(GeometryType::Point).is_ok());
    }

    #[test]
    fn text_that_is_no_json_or_has_trailing_characters_is_refused() {
        assert!(WrittenCoordinates::from_text("[1.0, ").is_err());
        assert!(WrittenCoordinates::from_text("[1.0, 2.0] x").is_err());
    }

    #[test]
    fn arbitrary_precision_coordinates_read_as_the_double_nearest_to_them() {
        let geometry = WrittenCoordinates::from_text("[0.100000000000000005,52.5]")
            .expect("valid JSON")
            .read_as(GeometryType::Point)
            .expect("a point");
        pretty_assertions::assert_eq!(geometry.to_geojson().value.type_name(), "Point");
    }

    /// The coordinates of the informative ETSI GS CIM 009 v1.9.1 Annex C.4
    /// example name a latitude of 100, which no payload geometry may either.
    #[test]
    fn a_reference_geometry_past_a_pole_is_refused_like_a_payload_geometry() {
        let refused = WrittenCoordinates::from_text("[-1, 100]").expect("valid JSON").read_as(GeometryType::Point);
        assert!(
            matches!(
                refused,
                Err(GeometryError::Position {
                    source: PositionError::LatitudeOutOfRange { .. },
                    ..
                })
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_coordinate_outside_the_finite_f64_domain_is_no_ordinate() {
        let refused = WrittenCoordinates::from_text("[1e400, 52.5]").expect("valid JSON").read_as(GeometryType::Point);
        assert!(matches!(refused, Err(GeometryError::NonNumericOrdinate { .. })), "{refused:?}");
    }

    #[test]
    fn coordinates_that_break_the_nesting_are_refused_with_their_geometry_error() {
        let refused = WrittenCoordinates::from_text("[1.0]").expect("valid JSON").read_as(GeometryType::Point);
        assert!(matches!(refused, Err(GeometryError::Position { .. })), "{refused:?}");
        let refused = WrittenCoordinates::from_text(r#""x""#).expect("valid JSON").read_as(GeometryType::Point);
        assert!(matches!(refused, Err(GeometryError::NotAnArray { .. })), "{refused:?}");
    }
}
