//! Decoding and encoding a [`GeometryForm`] as GeoJSON.
//!
//! The conversion goes straight between [`GeometryForm`] and
//! `geojson::GeometryValue`, never through `geo_types`. The `geojson` crate's
//! own `geo_types` conversion drops every ordinate past the second, because
//! `geo_types` is two-dimensional by construction, so routing through it would
//! throw altitude away before this crate ever saw it.

use crate::{
    form::{GeometryForm, PolygonRings},
    position::{Position, PositionError},
};
use geojson::{GeometryValue, Position as GeoJsonPosition};
use thiserror::Error;

/// Failures decoding a GeoJSON geometry into the domain model.
#[derive(Debug, Error)]
pub enum GeometryError {
    /// A position of the geometry had an ordinate count that carries no meaning
    /// under IETF RFC 7946 section 3.1.1.
    #[error(transparent)]
    Position(#[from] PositionError),
    /// The raw JSON value was not a GeoJSON geometry object at all: an unknown
    /// `type`, a missing or misshapen `coordinates` member, or an ordinate that
    /// is not a number.
    #[error("invalid GeoJSON geometry value: {0}")]
    GeojsonValue(#[from] serde_json::Error),
    /// The value is a GeoJSON GeometryCollection. ETSI GS CIM 009 v1.9.1 clause
    /// 4.7.1 allows a GeoProperty value every geometry RFC 7946 defines except
    /// this one, and clause 4.6.3 excludes it from reference geometries too.
    #[error(
        "GeometryCollection is not an allowed GeoProperty geometry: ETSI GS CIM 009 v1.9.1 clause 4.7.1 allows every RFC 7946 geometry except GeometryCollection"
    )]
    GeometryCollectionUnsupported,
}

impl GeometryForm {
    /// Decode from a parsed GeoJSON geometry value.
    ///
    /// A `bbox` or foreign member is not part of the geometry RFC 7946 section
    /// 3.1 defines and is not carried.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::Position`] if any position of the geometry
    /// carries fewer than two or more than three ordinates, and
    /// [`GeometryError::GeometryCollectionUnsupported`] for a GeometryCollection,
    /// which ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes.
    pub fn from_geojson(value: &GeometryValue) -> Result<GeometryForm, GeometryError> {
        match value {
            GeometryValue::Point { coordinates } => Ok(GeometryForm::Point(Position::try_from(coordinates)?)),
            GeometryValue::MultiPoint { coordinates } => Ok(GeometryForm::MultiPoint(decode_positions(coordinates)?)),
            GeometryValue::LineString { coordinates } => Ok(GeometryForm::LineString(decode_positions(coordinates)?)),
            GeometryValue::MultiLineString { coordinates } => {
                let line_strings = coordinates
                    .iter()
                    .map(|line_string| decode_positions(line_string))
                    .collect::<Result<Vec<Vec<Position>>, PositionError>>()?;
                Ok(GeometryForm::MultiLineString(line_strings))
            }
            GeometryValue::Polygon { coordinates } => Ok(GeometryForm::Polygon(decode_rings(coordinates)?)),
            GeometryValue::MultiPolygon { coordinates } => {
                let polygons = coordinates
                    .iter()
                    .map(|polygon| decode_rings(polygon))
                    .collect::<Result<Vec<PolygonRings>, PositionError>>()?;
                Ok(GeometryForm::MultiPolygon(polygons))
            }
            GeometryValue::GeometryCollection { geometries: _ } => Err(GeometryError::GeometryCollectionUnsupported),
        }
    }

    /// Encode as a GeoJSON geometry value.
    ///
    /// Infallible, and faithful to what was decoded: each position renders with
    /// the ordinate count it arrived with, and rings are not closed on the way
    /// out.
    pub fn to_geojson(&self) -> GeometryValue {
        match self {
            GeometryForm::Point(position) => GeometryValue::Point {
                coordinates: position.to_geojson(),
            },
            GeometryForm::MultiPoint(positions) => GeometryValue::MultiPoint {
                coordinates: encode_positions(positions),
            },
            GeometryForm::LineString(positions) => GeometryValue::LineString {
                coordinates: encode_positions(positions),
            },
            GeometryForm::MultiLineString(line_strings) => GeometryValue::MultiLineString {
                coordinates: line_strings.iter().map(|line_string| encode_positions(line_string)).collect(),
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

/// Decode a flat list of GeoJSON positions.
fn decode_positions(positions: &[GeoJsonPosition]) -> Result<Vec<Position>, PositionError> {
    positions.iter().map(Position::try_from).collect()
}

/// Decode the ring list of a GeoJSON Polygon.
fn decode_rings(rings: &[Vec<GeoJsonPosition>]) -> Result<PolygonRings, PositionError> {
    let rings = rings
        .iter()
        .map(|ring| decode_positions(ring))
        .collect::<Result<Vec<Vec<Position>>, PositionError>>()?;
    Ok(PolygonRings::new(rings))
}

/// Encode a flat list of positions as GeoJSON.
fn encode_positions(positions: &[Position]) -> Vec<GeoJsonPosition> {
    positions.iter().map(Position::to_geojson).collect()
}

/// Encode the rings of a Polygon as GeoJSON.
fn encode_rings(rings: &PolygonRings) -> Vec<Vec<GeoJsonPosition>> {
    rings.rings().iter().map(|ring| encode_positions(ring)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::GeometryDimension;

    fn decode(value: serde_json::Value) -> Result<GeometryForm, GeometryError> {
        let geometry: geojson::Geometry = serde_json::from_value(value)?;
        GeometryForm::from_geojson(&geometry.value)
    }

    #[test]
    fn a_point_with_four_ordinates_is_a_position_error() {
        let error = decode(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0] })).expect_err("four ordinates have no defined meaning");
        assert!(
            matches!(error, GeometryError::Position(PositionError::TooManyOrdinates { ordinates: 4 })),
            "got {error:?}"
        );
    }

    #[test]
    fn a_line_string_position_with_one_ordinate_is_a_position_error() {
        let error = decode(serde_json::json!({ "type": "LineString", "coordinates": [[1.0], [2.0, 3.0]] })).expect_err("one ordinate is not a position");
        assert!(
            matches!(error, GeometryError::Position(PositionError::TooFewOrdinates { ordinates: 1 })),
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
            unreachable!("expected a LineString");
        };
        pretty_assertions::assert_eq!(coordinates.first().map(GeoJsonPosition::as_slice), Some(&[0.0, 0.0][..]));
        pretty_assertions::assert_eq!(coordinates.last().map(GeoJsonPosition::as_slice), Some(&[1.0, 1.0, 50.0][..]));
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
