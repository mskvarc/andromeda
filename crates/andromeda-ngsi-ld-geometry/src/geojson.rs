//! Decoding and encoding a [`GeometryForm`] as `GeoJSON`.
//!
//! The conversion goes straight between [`GeometryForm`] and
//! `geojson::GeometryValue`, never through `geo_types`. The `geojson` crate's
//! own `geo_types` conversion drops every ordinate past the second, because
//! `geo_types` is two-dimensional by construction, so routing through it would
//! throw altitude away before this crate ever saw it.

use crate::{
    coordinate_path::CoordinatePath,
    form::{GeometryForm, PolygonRings},
    geometry_type::GeometryType,
    line_string_coordinates::{LineStringCoordinates, LineStringError},
    linear_ring::{LinearRing, LinearRingError},
    position::{Position, PositionError},
};
use geojson::{GeometryValue, Position as GeoJsonPosition};
use thiserror::Error;

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
    /// An expanded `coordinates` member did not nest the way its geometry type
    /// requires: a number where a position list belongs, or a list where a
    /// number does.
    #[error("GeoJSON coordinates do not nest as a {geometry_type} requires (at {at})")]
    Misnested {
        /// The geometry type whose nesting the coordinates break.
        geometry_type: GeometryType,
        /// Where the misplaced member sits in the geometry's `coordinates`.
        at: CoordinatePath,
    },
    /// The raw JSON value was not a `GeoJSON` geometry object at all: an unknown
    /// `type`, a missing or misshapen `coordinates` member, or an ordinate that
    /// is not a number.
    #[error("invalid GeoJSON geometry value: {0}")]
    GeojsonValue(#[from] serde_json::Error),
    /// A JSON coordinate is outside the finite binary64 domain used for
    /// spatial operations.
    #[error("GeoJSON coordinate cannot be represented as a finite 64-bit float")]
    NonFiniteCoordinate,
    /// The value is a `GeoJSON` `GeometryCollection`. ETSI GS CIM 009 v1.9.1 clause
    /// 4.7.1 allows a `GeoProperty` value every geometry RFC 7946 defines except
    /// this one, and clause 4.6.3 excludes it from reference geometries too.
    #[error("GeometryCollection is not an allowed GeoProperty geometry; every RFC 7946 geometry except GeometryCollection is accepted")]
    GeometryCollectionUnsupported,
}

impl GeometryForm {
    /// Decode from a parsed `GeoJSON` geometry value.
    ///
    /// A `bbox` or foreign member is not part of the geometry RFC 7946 section
    /// 3.1 defines and is not carried.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::Position`] if any position of the geometry
    /// carries fewer than two or more than three ordinates or lies off the
    /// CRS84 axes, [`GeometryError::LineString`] for a `LineString` of fewer
    /// than two positions, [`GeometryError::LinearRing`] for a Polygon ring
    /// that is open or holds fewer than four, and
    /// [`GeometryError::GeometryCollectionUnsupported`] for a `GeometryCollection`,
    /// which ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes.
    pub fn from_geojson(value: &GeometryValue) -> Result<GeometryForm, GeometryError> {
        match value {
            GeometryValue::Point { coordinates } => Ok(GeometryForm::Point(decode_position(coordinates, &[])?)),
            GeometryValue::MultiPoint { coordinates } => Ok(GeometryForm::MultiPoint(decode_positions(coordinates, &[])?)),
            GeometryValue::LineString { coordinates } => Ok(GeometryForm::LineString(decode_line_string(coordinates, &[])?)),
            GeometryValue::MultiLineString { coordinates } => {
                let line_strings = coordinates
                    .iter()
                    .enumerate()
                    .map(|(line_string, positions)| decode_line_string(positions, &[line_string]))
                    .collect::<Result<Vec<LineStringCoordinates>, GeometryError>>()?;
                Ok(GeometryForm::MultiLineString(line_strings))
            }
            GeometryValue::Polygon { coordinates } => Ok(GeometryForm::Polygon(decode_rings(coordinates, None)?)),
            GeometryValue::MultiPolygon { coordinates } => {
                let polygons = coordinates
                    .iter()
                    .enumerate()
                    .map(|(polygon, rings)| decode_rings(rings, Some(polygon)))
                    .collect::<Result<Vec<PolygonRings>, GeometryError>>()?;
                Ok(GeometryForm::MultiPolygon(polygons))
            }
            GeometryValue::GeometryCollection { geometries: _ } => Err(GeometryError::GeometryCollectionUnsupported),
        }
    }

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

/// Decode the position `enclosing` leads to, naming it on refusal.
fn decode_position(position: &GeoJsonPosition, enclosing: &[usize]) -> Result<Position, GeometryError> {
    Position::try_from(position).map_err(|source| GeometryError::Position {
        at: CoordinatePath::new(enclosing.to_vec()),
        source,
    })
}

/// Decode the flat list of `GeoJSON` positions `enclosing` leads to.
fn decode_positions(positions: &[GeoJsonPosition], enclosing: &[usize]) -> Result<Vec<Position>, GeometryError> {
    positions
        .iter()
        .enumerate()
        .map(|(index, position)| {
            Position::try_from(position).map_err(|source| GeometryError::Position {
                at: CoordinatePath::nested(enclosing, index),
                source,
            })
        })
        .collect()
}

/// Decode the positions of the `LineString` `enclosing` leads to.
fn decode_line_string(positions: &[GeoJsonPosition], enclosing: &[usize]) -> Result<LineStringCoordinates, GeometryError> {
    LineStringCoordinates::try_new(decode_positions(positions, enclosing)?).map_err(|source| GeometryError::LineString {
        at: CoordinatePath::new(enclosing.to_vec()),
        source,
    })
}

/// Decode the positions of the Polygon ring `enclosing` leads to.
fn decode_ring(positions: &[GeoJsonPosition], enclosing: &[usize]) -> Result<LinearRing, GeometryError> {
    LinearRing::try_new(decode_positions(positions, enclosing)?).map_err(|source| GeometryError::LinearRing {
        at: CoordinatePath::new(enclosing.to_vec()),
        source,
    })
}

/// Decode the ring list of a `GeoJSON` Polygon, standalone or as the member at
/// index `polygon` of a `MultiPolygon`.
fn decode_rings(rings: &[Vec<GeoJsonPosition>], polygon: Option<usize>) -> Result<PolygonRings, GeometryError> {
    let rings = rings
        .iter()
        .enumerate()
        .map(|(ring, positions)| match polygon {
            Some(polygon) => decode_ring(positions, &[polygon, ring]),
            None => decode_ring(positions, &[ring]),
        })
        .collect::<Result<Vec<LinearRing>, GeometryError>>()?;
    Ok(PolygonRings::new(rings))
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
    use geojson::{GeometryValue, Position as GeoJsonPosition};

    fn decode(value: serde_json::Value) -> Result<GeometryForm, GeometryError> {
        let geometry: geojson::Geometry = serde_json::from_value(value)?;
        GeometryForm::from_geojson(&geometry.value)
    }

    #[test]
    fn a_point_with_four_ordinates_is_a_position_error() {
        let error = decode(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0] })).expect_err("four ordinates have no defined meaning");
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
        let error = decode(serde_json::json!({ "type": "LineString", "coordinates": [[1.0], [2.0, 3.0]] })).expect_err("one ordinate is not a position");
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

    #[test]
    fn a_geometry_of_an_unknown_type_is_a_geojson_value_error() {
        let error = decode(serde_json::json!({ "type": "Nonsense", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::GeojsonValue(_)), "got {error:?}");
    }

    #[test]
    fn a_geometry_collection_is_refused_as_a_geoproperty_geometry() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7.1 allows every RFC 7946 geometry
        // except GeometryCollection.
        let error = decode(serde_json::json!({
            "type": "GeometryCollection",
            "geometries": [{ "type": "Point", "coordinates": [1.0, 2.0] }]
        }))
        .expect_err("a collection is not an allowed GeoProperty geometry");
        assert!(matches!(error, GeometryError::GeometryCollectionUnsupported), "got {error:?}");
    }

    #[test]
    fn a_mixed_arity_line_string_keeps_the_arity_of_each_position() {
        let form = decode(serde_json::json!({ "type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0, 50.0]] })).expect("mixed arity is valid GeoJSON");
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::TwoD);
        let GeometryValue::LineString { coordinates } = form.to_geojson() else {
            panic!("expected a LineString");
        };
        pretty_assertions::assert_eq!(coordinates.first().map(GeoJsonPosition::as_slice), Some(&[0.0, 0.0][..]));
        pretty_assertions::assert_eq!(coordinates.last().map(GeoJsonPosition::as_slice), Some(&[1.0, 1.0, 50.0][..]));
    }

    #[test]
    fn a_line_string_of_one_position_is_refused_with_its_place() {
        let error = decode(serde_json::json!({ "type": "LineString", "coordinates": [[1.0, 2.0]] })).expect_err("one position traces no line");
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
        let error = decode(serde_json::json!({ "type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], []] }))
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
        let error = decode(serde_json::json!({
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
        let error = decode(serde_json::json!({
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
        assert!(decode(serde_json::json!({ "type": "Polygon", "coordinates": [] })).is_ok());
    }

    #[test]
    fn a_position_off_the_axes_is_named_before_the_ring_it_sits_in() {
        let error = decode(serde_json::json!({ "type": "Polygon", "coordinates": [[[0.0, 0.0], [0.0, 95.0]]] })).expect_err("latitude 95 is no position");
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
        let form = decode(serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]]]
        }))
        .expect("polygon decodes");
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::ThreeD);
    }
}
