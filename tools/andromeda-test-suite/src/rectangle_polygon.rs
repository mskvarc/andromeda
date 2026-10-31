//! The polygon a test draws as a rectangle: one closed exterior ring walked counter-clockwise from the south-west
//! corner, the shape of every box the area tests query and register.

use ngsi_ld::model::geometry::{GeometryError, LinearRing, Polygon, Position, Shape};

/// The rectangle between longitudes `west` and `east` and latitudes `south` and `north`, closed on its first corner.
///
/// # Errors
///
/// Returns [`GeometryError`] when a corner lies outside the WGS 84 ranges.
pub fn rectangle_polygon(west: f64, south: f64, east: f64, north: f64) -> Result<Shape, GeometryError> {
    let ring = LinearRing::try_new(vec![
        Position::try_new(west, south)?,
        Position::try_new(east, south)?,
        Position::try_new(east, north)?,
        Position::try_new(west, north)?,
        Position::try_new(west, south)?,
    ])?;
    Ok(Shape::Polygon(Polygon::try_new(vec![ring])?))
}

#[cfg(test)]
mod tests {
    use crate::rectangle_polygon::rectangle_polygon;
    use googletest::prelude::*;
    use ngsi_ld::model::geometry::Shape;

    #[gtest]
    fn the_ring_closes_on_its_first_corner() -> Result<()> {
        let shape = rectangle_polygon(-4.2, 39.9, -3.2, 40.9)?;
        let Shape::Polygon(polygon) = shape else {
            return fail!("the rectangle is a polygon");
        };
        let ring = polygon.rings().first().or_fail()?;
        verify_that!(ring.positions(), len(eq(5)))
    }

    #[gtest]
    fn a_corner_outside_the_ranges_is_refused() {
        expect_that!(rectangle_polygon(-200.0, 0.0, 10.0, 10.0), err(anything()));
    }
}
