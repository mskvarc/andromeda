//! The geometries the conformance tests write as `GeoProperty` values and geo-query shapes.

use googletest::Result;
use ngsi_ld::model::geometry::{LinearRing, Polygon, Position, Shape};

/// A point at `longitude` and `latitude`, as a geometry value.
pub(crate) fn point(longitude: f64, latitude: f64) -> Result<Shape> {
    Ok(Shape::Point(Position::try_new(longitude, latitude)?))
}

/// The polygon of `corners`, a closed ring of (longitude, latitude) positions.
pub(crate) fn polygon(corners: &[(f64, f64)]) -> Result<Shape> {
    let ring = LinearRing::try_new(
        corners
            .iter()
            .map(|&(longitude, latitude)| Ok(Position::try_new(longitude, latitude)?))
            .collect::<Result<Vec<_>>>()?,
    )?;
    Ok(Shape::Polygon(Polygon::try_new(vec![ring])?))
}
