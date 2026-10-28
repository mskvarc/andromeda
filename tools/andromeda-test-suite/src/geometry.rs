//! Geometries between the model and the `GeoJSON` a document writes (RFC 7946): positions written as a table of
//! ordinates, where a line or ring of many positions reads as `[longitude, latitude]` pairs and becomes the model's
//! positions, each checked against the WGS 84 ranges (section 4); and a geometry as the `GeoJSON` object it is
//! written as, so a matcher reads its members by path.

use crate::value::{from_json, object};
use ngsi_ld::model::{
    Value,
    geometry::{Geometry, GeometryError, Polygon, Position, Shape},
};

/// The positions of `ordinates`, `[longitude, latitude]` each.
///
/// # Errors
///
/// Returns [`GeometryError`] for the first pair outside the WGS 84 ranges.
pub fn positions(ordinates: &[[f64; 2]]) -> Result<Vec<Position>, GeometryError> {
    ordinates.iter().map(|&[longitude, latitude]| Position::try_new(longitude, latitude)).collect()
}

/// A position as the array of its ordinates.
fn position_value(position: &Position) -> Value {
    let ordinates = [Some(position.longitude()), Some(position.latitude()), position.altitude()];
    Value::Array(ordinates.into_iter().flatten().map(|ordinate| Value::Number(ordinate.clone())).collect())
}

/// Positions as an array of position arrays.
fn positions_value(positions: &[Position]) -> Value {
    Value::Array(positions.iter().map(position_value).collect())
}

/// A polygon as the array of its rings.
fn polygon_value(polygon: &Polygon) -> Value {
    Value::Array(polygon.rings().iter().map(|ring| positions_value(ring.positions())).collect())
}

/// A geometry as the `GeoJSON` object it is written as (RFC 7946 section 3.1), so its members are read by path.
pub(crate) fn geometry_value(geometry: &Geometry) -> Value {
    let mut value = geometry_object(geometry);
    if let Value::Object(members) = &mut value {
        for (name, raw) in geometry.foreign_members().iter() {
            let member = serde_json::from_str::<serde_json::Value>(&raw.to_string())
                .ok()
                .and_then(|json| from_json(&json));
            if let Some(member) = member {
                members.insert(name.to_string(), member);
            }
        }
    }
    value
}

/// A geometry's `type`, `coordinates` and `bbox` as the `GeoJSON` object they are written as.
fn geometry_object(geometry: &Geometry) -> Value {
    let shape = geometry.shape();
    let coordinates = match shape {
        Shape::Point(position) => position_value(position),
        Shape::MultiPoint(positions) => positions_value(positions),
        Shape::LineString(line) => positions_value(line.positions()),
        Shape::MultiLineString(lines) => Value::Array(lines.iter().map(|line| positions_value(line.positions())).collect()),
        Shape::Polygon(polygon) => polygon_value(polygon),
        Shape::MultiPolygon(polygons) => Value::Array(polygons.iter().map(polygon_value).collect()),
    };
    let name: &'static str = shape.into();
    match geometry.bbox() {
        None => object([("type", Value::from(name)), ("coordinates", coordinates)]),
        Some(bbox) => {
            let corners = [bbox.south_west(), bbox.north_east()];
            let mut ordinates: Vec<Value> = corners
                .iter()
                .flat_map(|corner| [corner.longitude(), corner.latitude()])
                .map(|ordinate| Value::Number(ordinate.clone()))
                .collect();
            if let (Some(low), Some(high)) = (bbox.south_west().altitude(), bbox.north_east().altitude()) {
                // RFC 7946 section 5 writes a three-dimensional box as both lows, then both highs.
                ordinates = [
                    corners[0].longitude(),
                    corners[0].latitude(),
                    low,
                    corners[1].longitude(),
                    corners[1].latitude(),
                    high,
                ]
                .into_iter()
                .map(|ordinate| Value::Number(ordinate.clone()))
                .collect();
            }
            object([("type", Value::from(name)), ("coordinates", coordinates), ("bbox", Value::Array(ordinates))])
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::positions;
    use googletest::prelude::*;
    use ngsi_ld::model::geometry::Position;

    #[gtest]
    fn each_pair_becomes_a_position_in_order() -> Result<()> {
        verify_that!(
            positions(&[[1.0, 2.0], [3.0, 4.0]])?,
            eq(&vec![Position::try_new(1.0, 2.0)?, Position::try_new(3.0, 4.0)?])
        )
    }

    #[gtest]
    fn a_latitude_outside_the_range_is_refused() -> Result<()> {
        verify_that!(positions(&[[1.0, 91.0]]), err(anything()))
    }
}
