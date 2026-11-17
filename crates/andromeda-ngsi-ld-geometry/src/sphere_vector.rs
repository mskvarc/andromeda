//! A position as a vector of three-dimensional space, the form great-circle
//! geometry is computed in.
//!
//! On the unit sphere a position is the vector from the centre of the earth,
//! a great circle is the plane through the centre normal to the cross product
//! of two of its points, and the side of a great circle a point lies on is the
//! sign of its dot product with that normal. [`crate::great_circle`],
//! [`crate::arc_meeting`] and [`crate::spherical_polygon`] read edges this way.

use geo::Coord;

/// A vector of three-dimensional space, a point of the unit sphere when its
/// length is 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SphereVector([f64; 3]);

impl SphereVector {
    /// The north pole.
    pub(crate) const NORTH_POLE: SphereVector = SphereVector([0.0, 0.0, 1.0]);

    /// The unit vector of a longitude/latitude position, in degrees.
    pub(crate) fn of(position: Coord<f64>) -> SphereVector {
        let (longitude, latitude) = (position.x.to_radians(), position.y.to_radians());
        SphereVector([latitude.cos() * longitude.cos(), latitude.cos() * longitude.sin(), latitude.sin()])
    }

    /// The vector with components `x`, `y` and `z`.
    pub(crate) const fn new(x: f64, y: f64, z: f64) -> SphereVector {
        SphereVector([x, y, z])
    }

    /// The cross product, normal to the plane of the two vectors.
    pub(crate) const fn cross(self, other: SphereVector) -> SphereVector {
        let ([a0, a1, a2], [b0, b1, b2]) = (self.0, other.0);
        SphereVector([a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0])
    }

    /// The dot product.
    pub(crate) const fn dot(self, other: SphereVector) -> f64 {
        let ([a0, a1, a2], [b0, b1, b2]) = (self.0, other.0);
        a0 * b0 + a1 * b1 + a2 * b2
    }

    /// The vector scaled by `factor`.
    pub(crate) const fn scaled(self, factor: f64) -> SphereVector {
        let [x, y, z] = self.0;
        SphereVector([x * factor, y * factor, z * factor])
    }

    /// The difference of two vectors.
    pub(crate) const fn minus(self, other: SphereVector) -> SphereVector {
        let ([a0, a1, a2], [b0, b1, b2]) = (self.0, other.0);
        SphereVector([a0 - b0, a1 - b1, a2 - b2])
    }

    /// The vector pointing the other way: on the unit sphere, the antipode.
    pub(crate) const fn antipode(self) -> SphereVector {
        self.scaled(-1.0)
    }

    /// The unit vector in the same direction, or `None` for a vector too short
    /// to have one: the cross product of two coincident or antipodal
    /// positions.
    pub(crate) fn normalized(self) -> Option<SphereVector> {
        let length = self.dot(self).sqrt();
        (length >= f64::EPSILON).then(|| self.scaled(length.recip()))
    }

    /// The `z` component: on the unit sphere, the sine of the latitude.
    pub(crate) const fn z(self) -> f64 {
        self.0[2]
    }

    /// The component-wise least and greatest of two vectors.
    pub(crate) const fn extremes(self, other: SphereVector) -> (SphereVector, SphereVector) {
        let ([a0, a1, a2], [b0, b1, b2]) = (self.0, other.0);
        (
            SphereVector([a0.min(b0), a1.min(b1), a2.min(b2)]),
            SphereVector([a0.max(b0), a1.max(b1), a2.max(b2)]),
        )
    }

    /// The midpoint of two vectors.
    pub(crate) const fn midpoint(self, other: SphereVector) -> SphereVector {
        let ([a0, a1, a2], [b0, b1, b2]) = (self.0, other.0);
        SphereVector([f64::midpoint(a0, b0), f64::midpoint(a1, b1), f64::midpoint(a2, b2)])
    }

    /// The latitude, in degrees, of a unit vector.
    pub(crate) fn latitude(self) -> f64 {
        self.z().clamp(-1.0, 1.0).asin().to_degrees()
    }
}

#[cfg(test)]
mod tests {
    use crate::sphere_vector::SphereVector;
    use geo::Coord;

    fn at(longitude: f64, latitude: f64) -> SphereVector {
        SphereVector::of(Coord { x: longitude, y: latitude })
    }

    fn close(a: SphereVector, b: SphereVector) -> bool {
        a.minus(b).dot(a.minus(b)) < 1e-24
    }

    #[test]
    fn a_position_is_a_unit_vector_at_its_latitude() {
        let vector = at(37.0, -21.5);
        assert!((vector.dot(vector) - 1.0).abs() < 1e-15);
        assert!((vector.latitude() + 21.5).abs() < 1e-12);
    }

    #[test]
    fn the_antimeridian_seen_from_either_side_is_one_position() {
        assert!(close(at(180.0, 10.0), at(-180.0, 10.0)));
    }

    #[test]
    fn the_cross_product_of_two_equatorial_positions_points_at_a_pole() {
        let normal = at(0.0, 0.0).cross(at(90.0, 0.0)).normalized().expect("the positions are a quarter turn apart");
        assert!(close(normal, SphereVector::NORTH_POLE));
    }

    #[test]
    fn coincident_and_antipodal_positions_span_no_plane() {
        assert!(at(10.0, 20.0).cross(at(10.0, 20.0)).normalized().is_none());
        assert!(at(10.0, 20.0).cross(at(-170.0, -20.0)).normalized().is_none());
    }

    #[test]
    fn the_antipode_of_a_position_lies_across_the_earth() {
        assert!(close(at(10.0, 20.0).antipode(), at(-170.0, -20.0)));
    }
}
