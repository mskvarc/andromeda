//! The shape of a geometry: its `GeoJSON` type together with its positions.
//!
//! This is the wire form held verbatim. IETF RFC 7946 section 3.1.6 requires
//! the first and last position of a linear ring to be identical, but a ring
//! received open is kept open here rather than silently closed: what a client
//! sent is what a client reads back, and closing a ring is a concern of the
//! evaluation projection in [`crate::planar`], which builds its own closed copy.

use crate::{dimension::GeometryDimension, position::Position};

/// The rings of a `GeoJSON` Polygon: an exterior ring followed by zero or more
/// interior rings (IETF RFC 7946 section 3.1.6).
///
/// A Polygon with no rings at all is representable, because RFC 7946 permits
/// `"coordinates": []`.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonRings(Vec<Vec<Position>>);

impl PolygonRings {
    /// Build from the ring list of a `GeoJSON` Polygon, exterior ring first.
    #[must_use]
    pub const fn new(rings: Vec<Vec<Position>>) -> PolygonRings {
        PolygonRings(rings)
    }

    /// The rings, exterior ring first.
    #[must_use]
    pub fn rings(&self) -> &[Vec<Position>] {
        &self.0
    }
}

/// A `GeoJSON` geometry object's type and coordinates (IETF RFC 7946 section 3.1),
/// which ETSI GS CIM 009 v1.9.1 clause 4.7 makes the value of a `GeoProperty`.
///
/// There is deliberately no `GeometryCollection` variant. Clause 4.7.1 allows a
/// `GeoProperty` value every geometry RFC 7946 defines *except* `GeometryCollection`,
/// and clause 4.6.3 excludes it from reference geometries as well, so a
/// collection is not a state this type is allowed to reach.
///
/// Every variant holds [`Position`]s, so altitude survives a decode/encode
/// round trip for each position that carried one.
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryForm {
    /// A single position.
    Point(Position),
    /// An array of positions.
    MultiPoint(Vec<Position>),
    /// An array of two or more positions.
    LineString(Vec<Position>),
    /// An array of `LineString` coordinate arrays.
    MultiLineString(Vec<Vec<Position>>),
    /// An array of linear ring coordinate arrays.
    Polygon(PolygonRings),
    /// An array of Polygon coordinate arrays.
    MultiPolygon(Vec<PolygonRings>),
}

impl GeometryForm {
    /// Whether every position of this geometry carries an altitude.
    ///
    /// [`GeometryDimension::ThreeD`] only when the geometry holds at least one
    /// position and all of them carry a third ordinate. A geometry with no
    /// positions has no altitudes to speak of, and one whose positions disagree
    /// is only partly elevated; both are [`GeometryDimension::TwoD`].
    #[must_use]
    pub fn dimension(&self) -> GeometryDimension {
        let mut census = PositionCensus::default();
        self.take_census(&mut census);
        census.dimension()
    }

    /// Fold every position of this geometry into the running `census`.
    fn take_census(&self, census: &mut PositionCensus) {
        match self {
            GeometryForm::Point(position) => census.record(position),
            GeometryForm::MultiPoint(positions) | GeometryForm::LineString(positions) => positions.iter().for_each(|position| census.record(position)),
            GeometryForm::MultiLineString(line_strings) => line_strings.iter().flatten().for_each(|position| census.record(position)),
            GeometryForm::Polygon(rings) => rings.rings().iter().flatten().for_each(|position| census.record(position)),
            GeometryForm::MultiPolygon(polygons) => polygons
                .iter()
                .flat_map(|polygon| polygon.rings().iter().flatten())
                .for_each(|position| census.record(position)),
        }
    }
}

/// Running totals over the positions of a geometry, from which its
/// dimensionality follows.
#[derive(Debug, Default, Clone, Copy)]
struct PositionCensus {
    /// Positions seen so far.
    positions: usize,
    /// Positions seen so far that carry an altitude.
    elevated: usize,
}

impl PositionCensus {
    /// Count one position.
    const fn record(&mut self, position: &Position) {
        self.positions += 1;
        match position.dimension() {
            GeometryDimension::ThreeD => self.elevated += 1,
            GeometryDimension::TwoD => (),
        }
    }

    /// The dimensionality the counted positions add up to.
    const fn dimension(&self) -> GeometryDimension {
        if self.positions > 0 && self.positions == self.elevated {
            GeometryDimension::ThreeD
        } else {
            GeometryDimension::TwoD
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn tall(longitude: f64, latitude: f64, altitude: f64) -> Position {
        Position::try_new(longitude, latitude, Some(altitude)).expect("a test position lies on the CRS84 axes")
    }

    #[test]
    fn a_geometry_whose_positions_all_carry_an_altitude_is_three_dimensional() {
        let form = GeometryForm::Polygon(PolygonRings::new(vec![vec![tall(0.0, 0.0, 5.0), tall(1.0, 0.0, 5.0), tall(1.0, 1.0, 5.0)]]));
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::ThreeD);
    }

    #[test]
    fn a_geometry_with_mixed_position_arity_is_two_dimensional() {
        let form = GeometryForm::LineString(vec![flat(0.0, 0.0), tall(1.0, 1.0, 50.0)]);
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_geometry_with_no_positions_is_two_dimensional() {
        let form = GeometryForm::MultiPoint(Vec::new());
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_multi_polygon_is_three_dimensional_only_when_every_ring_vertex_is() {
        let elevated = GeometryForm::MultiPolygon(vec![PolygonRings::new(vec![vec![
            tall(0.0, 0.0, 1.0),
            tall(1.0, 0.0, 1.0),
            tall(1.0, 1.0, 1.0),
        ]])]);
        let mixed = GeometryForm::MultiPolygon(vec![PolygonRings::new(vec![vec![tall(0.0, 0.0, 1.0), flat(1.0, 0.0), tall(1.0, 1.0, 1.0)]])]);
        pretty_assertions::assert_eq!(elevated.dimension(), GeometryDimension::ThreeD);
        pretty_assertions::assert_eq!(mixed.dimension(), GeometryDimension::TwoD);
    }
}
