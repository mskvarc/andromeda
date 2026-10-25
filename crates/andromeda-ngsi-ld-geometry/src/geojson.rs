//! Encoding a [`GeometryForm`] as `GeoJSON`, and why a value is refused as
//! one.
//!
//! The encoding goes straight from [`GeometryForm`] to
//! `geojson::GeometryValue`, never through `geo_types`. The `geojson` crate's
//! own `geo_types` conversion drops every ordinate past the second, because
//! `geo_types` is two-dimensional by construction, so routing through it would
//! throw altitude away. Decoding reads a compact geometry object straight from
//! a deserializer (see [`crate::written_geometry`]) or from its text (see
//! [`crate::compact_geojson`]), the two sharing one reading of its members.

use crate::{
    bounding_box::BoundingBoxError,
    coordinate_path::CoordinatePath,
    form::{GeometryForm, PolygonRings},
    line_string_coordinates::LineStringError,
    linear_ring::LinearRingError,
    position::{Position, PositionError},
};
use andromeda_json_literal::admission::JsonLiteralError;
use geojson::{GeometryValue, Position as GeoJsonPosition};
use strum::Display;
use thiserror::Error;

/// What a `GeoJSON` coordinate array was expected to hold where a non-array
/// stood instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub enum CoordinateShape {
    /// One position.
    #[strum(serialize = "a position")]
    Position,
    /// A list of positions.
    #[strum(serialize = "a list of positions")]
    Positions,
    /// A list of lines or rings, or of polygons.
    #[strum(serialize = "a list")]
    List,
}

/// Failures decoding a `GeoJSON` geometry into the domain model.
#[derive(Debug, Error)]
pub enum GeometryError {
    /// A position of the geometry had an ordinate count that carries no meaning
    /// under IETF RFC 7946 section 3.1.1, a member that is not a number, or an
    /// ordinate off the CRS84 axes of section 4.
    #[error("{source} (at {at})")]
    Position {
        /// Where the refused position sits in the geometry's `coordinates`.
        at: CoordinatePath,
        /// What is wrong with it.
        #[source]
        source: PositionError,
    },
    /// A `LineString`, alone or inside a `MultiLineString`, held fewer than the
    /// two positions RFC 7946 section 3.1.4 requires.
    #[error("{source} (at {at})")]
    LineString {
        /// Where the refused line sits in the geometry's `coordinates`.
        at: CoordinatePath,
        /// What is wrong with it.
        #[source]
        source: LineStringError,
    },
    /// A Polygon ring was open or held fewer than the four positions RFC 7946
    /// section 3.1.6 requires.
    #[error("{source} (at {at})")]
    LinearRing {
        /// Where the refused ring sits in the geometry's `coordinates`.
        at: CoordinatePath,
        /// What is wrong with it.
        #[source]
        source: LinearRingError,
    },
    /// A position carried a different number of ordinates than the first
    /// position of the same geometry.
    #[error(
        "GeoJSON position carries a different number of ordinates than the geometry's first position; every position of one geometry is two-dimensional or every one carries an altitude (at {at})"
    )]
    MixedArity {
        /// Where the first disagreeing position sits in the geometry's
        /// `coordinates`.
        at: CoordinatePath,
    },
    /// The value is not a JSON object, so no `GeoJSON` geometry (IETF RFC 7946
    /// section 3.1).
    #[error("a GeoJSON geometry must be an object")]
    NotAnObject,
    /// A geometry encoded as a JSON string (ETSI GS CIM 009 v1.9.1 clause
    /// 4.7.2) is not a JSON text.
    #[error("the string-encoded GeoJSON geometry is not JSON: {0}")]
    EncodedNotJson(#[source] serde_json::Error),
    /// The geometry object carries no string `type` member.
    #[error("a GeoJSON geometry must carry a string `type`")]
    MissingType,
    /// The `type` member names no geometry type RFC 7946 section 1.4 defines.
    #[error("the GeoJSON geometry `type` is not one RFC 7946 defines")]
    UnknownType(#[source] strum::ParseError),
    /// The geometry object carries no `coordinates` member.
    #[error("a GeoJSON geometry must carry `coordinates`")]
    MissingCoordinates,
    /// A `GeometryCollection` carries no `geometries` array.
    #[error("a GeoJSON GeometryCollection must carry a `geometries` array")]
    MissingGeometries,
    /// The `bbox` member is no RFC 7946 section 5 bounding box.
    #[error(transparent)]
    InvalidBbox(#[from] BoundingBoxError),
    /// A `coordinates` member held a non-array where an array belongs.
    #[error("GeoJSON coordinates hold a non-array where {expected} belongs (at {at})")]
    NotAnArray {
        /// What the array was to hold.
        expected: CoordinateShape,
        /// Where the non-array sits in the geometry's `coordinates`.
        at: CoordinatePath,
    },
    /// The value is a `GeoJSON` `GeometryCollection`. ETSI GS CIM 009 v1.9.1 clause
    /// 4.7.1 allows a `GeoProperty` value every geometry RFC 7946 defines except
    /// this one, and clause 4.6.3 excludes it from reference geometries too.
    #[error("GeometryCollection is not an allowed GeoProperty geometry; every RFC 7946 geometry except GeometryCollection is accepted")]
    GeometryCollectionUnsupported,
    /// A foreign member (IETF RFC 7946 section 6.1) holds JSON the broker does
    /// not keep.
    #[error("a foreign member of the GeoJSON geometry is not admissible JSON: {0}")]
    ForeignMember(#[source] JsonLiteralError),
}

impl GeometryForm {
    /// Encode as a `GeoJSON` geometry value.
    ///
    /// Infallible, and faithful to what was decoded: each position renders with
    /// the ordinate count it arrived with.
    pub fn to_geojson(&self) -> GeometryValue {
        match self {
            GeometryForm::Point(position) => GeometryValue::Point {
                coordinates: position.to_geojson(),
            },
            GeometryForm::MultiPoint(positions) => GeometryValue::MultiPoint {
                coordinates: encode_positions(positions),
            },
            GeometryForm::LineString(line_string) => GeometryValue::LineString {
                coordinates: encode_positions(line_string.positions()),
            },
            GeometryForm::MultiLineString(line_strings) => GeometryValue::MultiLineString {
                coordinates: line_strings.iter().map(|line_string| encode_positions(line_string.positions())).collect(),
            },
            GeometryForm::Polygon(rings) => GeometryValue::Polygon {
                coordinates: encode_rings(rings),
            },
            GeometryForm::MultiPolygon(polygons) => GeometryValue::MultiPolygon {
                coordinates: polygons.iter().map(encode_rings).collect(),
            },
        }
    }
}

/// Encode a flat list of positions as `GeoJSON`.
fn encode_positions(positions: &[Position]) -> Vec<GeoJsonPosition> {
    positions.iter().map(Position::to_geojson).collect()
}

/// Encode the rings of a Polygon as `GeoJSON`.
fn encode_rings(rings: &PolygonRings) -> Vec<Vec<GeoJsonPosition>> {
    rings.rings().iter().map(|ring| encode_positions(ring.positions())).collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        coordinate_path::CoordinatePath,
        dimension::GeometryDimension,
        form::GeometryForm,
        geojson::GeometryError,
        line_string_coordinates::LineStringError,
        linear_ring::LinearRingError,
        position::PositionError,
    };

    /// Read `value` as the compact `GeoJSON` geometry every write path reads.
    fn decode(value: &serde_json::Value) -> Result<GeometryForm, GeometryError> {
        GeometryForm::from_compact_text(&value.to_string())
    }

    #[test]
    fn a_point_with_four_ordinates_is_a_position_error() {
        let error = decode(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0] })).expect_err("four ordinates have no defined meaning");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::TooManyOrdinates { ordinates: 4 },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_line_string_position_with_one_ordinate_is_a_position_error() {
        let error = decode(&serde_json::json!({ "type": "LineString", "coordinates": [[1.0], [2.0, 3.0]] })).expect_err("one ordinate is not a position");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::TooFewOrdinates { ordinates: 1 },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    /// The refusal names no part of the client's value: the unknown token
    /// is not echoed back.
    #[test]
    fn a_geometry_of_an_unknown_type_is_refused_without_echoing_it() {
        let error = decode(&serde_json::json!({ "type": "Nonsense<script>", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::UnknownType(_)), "got {error:?}");
        pretty_assertions::assert_eq!(error.to_string(), "the GeoJSON geometry `type` is not one RFC 7946 defines");
    }

    #[test]
    fn a_geometry_collection_is_refused_as_a_geoproperty_geometry() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7.1 allows every RFC 7946 geometry
        // except GeometryCollection.
        let error = decode(&serde_json::json!({
            "type": "GeometryCollection",
            "geometries": [{ "type": "Point", "coordinates": [1.0, 2.0] }]
        }))
        .expect_err("a collection is not an allowed GeoProperty geometry");
        assert!(matches!(error, GeometryError::GeometryCollectionUnsupported), "got {error:?}");
    }

    #[test]
    fn a_mixed_arity_line_string_is_refused_at_the_disagreeing_position() {
        let error =
            decode(&serde_json::json!({ "type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0, 50.0]] })).expect_err("positions disagree on altitude");
        assert!(
            matches!(error, GeometryError::MixedArity { ref at } if *at == CoordinatePath::new(vec![1])),
            "got {error:?}"
        );
    }

    #[test]
    fn a_line_string_of_one_position_is_refused_with_its_place() {
        let error = decode(&serde_json::json!({ "type": "LineString", "coordinates": [[1.0, 2.0]] })).expect_err("one position traces no line");
        assert!(
            matches!(
                error,
                GeometryError::LineString { ref at, source: LineStringError::TooFewPositions { positions: 1 } } if *at == CoordinatePath::default()
            ),
            "got {error:?}"
        );
        pretty_assertions::assert_eq!(
            error.to_string(),
            "GeoJSON LineString has 1 of the two or more positions RFC 7946 section 3.1.4 requires (at coordinates)"
        );
    }

    #[test]
    fn a_short_member_of_a_multi_line_string_is_refused_with_its_index() {
        let error = decode(&serde_json::json!({ "type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], []] }))
            .expect_err("an empty member traces no line");
        assert!(
            matches!(
                error,
                GeometryError::LineString { ref at, source: LineStringError::TooFewPositions { positions: 0 } } if *at == CoordinatePath::new(vec![1])
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn an_open_polygon_ring_is_refused_with_its_index() {
        let error = decode(&serde_json::json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
                [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]]
            ]
        }))
        .expect_err("the hole is open");
        assert!(
            matches!(error, GeometryError::LinearRing { ref at, source: LinearRingError::Open } if *at == CoordinatePath::new(vec![1])),
            "got {error:?}"
        );
        pretty_assertions::assert_eq!(
            error.to_string(),
            "GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first, altitude included (at coordinates[1])"
        );
    }

    #[test]
    fn a_short_ring_of_a_later_polygon_is_refused_with_both_indices() {
        let error = decode(&serde_json::json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [[[5.0, 5.0], [6.0, 5.0], [5.0, 5.0]]]
            ]
        }))
        .expect_err("three positions enclose nothing");
        assert!(
            matches!(
                error,
                GeometryError::LinearRing { ref at, source: LinearRingError::TooFewPositions { positions: 3 } } if *at == CoordinatePath::new(vec![1, 0])
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_polygon_with_no_rings_decodes() {
        // RFC 7946 section 3.1 lets empty coordinates stand for a null geometry.
        assert!(decode(&serde_json::json!({ "type": "Polygon", "coordinates": [] })).is_ok());
    }

    #[test]
    fn a_position_off_the_axes_is_named_before_the_ring_it_sits_in() {
        let error = decode(&serde_json::json!({ "type": "Polygon", "coordinates": [[[0.0, 0.0], [0.0, 95.0]]] })).expect_err("latitude 95 is no position");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::LatitudeOutOfRange { .. },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_fully_elevated_polygon_is_three_dimensional() {
        let form = decode(&serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]]]
        }))
        .expect("polygon decodes");
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::ThreeD);
    }
}
