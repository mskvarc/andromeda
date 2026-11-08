//! The `geojson` crate's reading of a geometry, kept as the oracle the compact
//! reading is compared against.
//!
//! Every write path reads a geometry in place (see [`crate::compact_geojson`]);
//! this reading decodes it through the `geojson` crate first and is never used
//! outside the tests, which assert that both accept the same geometries into
//! the same form and refuse the others alike.

use crate::{
    coordinate_path::CoordinatePath,
    form::{GeometryForm, PolygonRings},
    geojson::GeometryError,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use geojson::{Geometry as GeoJsonGeometry, GeometryValue, Position as GeoJsonPosition};
use serde_json::Value;
use thiserror::Error;

/// Why the oracle refused a value.
#[derive(Debug, Error)]
pub(crate) enum OracleRefusal {
    /// The `geojson` crate does not read the value as a geometry object.
    #[error("not a GeoJSON geometry: {0}")]
    NotGeojson(#[from] serde_json::Error),
    /// The geometry is one the domain model refuses.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// Read `value` through the `geojson` crate.
pub(crate) fn through_geojson(value: &Value) -> Result<GeometryForm, OracleRefusal> {
    let geometry: GeoJsonGeometry = serde_json::from_value(value.clone())?; // clone: from_value takes its input by value, and the caller keeps its copy
    Ok(decode(&geometry.value)?.with_uniform_arity()?)
}

/// Decode a parsed `geojson` geometry value into the domain model.
fn decode(value: &GeometryValue) -> Result<GeometryForm, GeometryError> {
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

/// Decode the position `enclosing` leads to, naming it on refusal.
fn decode_position(position: &GeoJsonPosition, enclosing: &[usize]) -> Result<Position, GeometryError> {
    Position::try_from(position.as_slice()).map_err(|source| GeometryError::Position {
        at: CoordinatePath::new(enclosing.to_vec()),
        source,
    })
}

/// Decode the flat list of positions `enclosing` leads to.
fn decode_positions(positions: &[GeoJsonPosition], enclosing: &[usize]) -> Result<Vec<Position>, GeometryError> {
    positions
        .iter()
        .enumerate()
        .map(|(index, position)| {
            Position::try_from(position.as_slice()).map_err(|source| GeometryError::Position {
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

/// Decode the ring list of a Polygon, standalone or as the member at index
/// `polygon` of a `MultiPolygon`.
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
