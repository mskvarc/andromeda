//! The shape of a geometry: its `GeoJSON` type together with its positions.
//!
//! This is the wire form held verbatim, down to the altitude of every position.
//! Its structure is the one IETF RFC 7946 section 3.1 requires and nothing
//! looser: a `LineString` is a [`LineStringCoordinates`] of two or more
//! positions, and every ring of a Polygon a closed [`LinearRing`] of four or
//! more, so a geometry that decoded is one every consumer can use as it stands.

use crate::{
    dimension::GeometryDimension,
    geometry_census::PositionCount,
    geometry_type::GeometryType,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};

/// The rings of a `GeoJSON` Polygon: an exterior ring followed by zero or more
/// interior rings (IETF RFC 7946 section 3.1.6).
///
/// A Polygon with no rings at all is representable: section 3.1.6 asks for
/// "an array of linear ring coordinate arrays", which an empty array is, and
/// section 3.1 lets a processor read empty `coordinates` as a null geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonRings(Vec<LinearRing>);

impl PolygonRings {
    /// Build from the ring list of a `GeoJSON` Polygon, exterior ring first.
    #[must_use]
    pub const fn new(rings: Vec<LinearRing>) -> PolygonRings {
        PolygonRings(rings)
    }

    /// The rings, exterior ring first.
    #[must_use]
    pub fn rings(&self) -> &[LinearRing] {
        &self.0
    }

    /// Every position of every ring, exterior ring first.
    fn positions(&self) -> impl Iterator<Item = &Position> {
        self.0.iter().flat_map(LinearRing::positions)
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
    LineString(LineStringCoordinates),
    /// An array of `LineString` coordinate arrays.
    MultiLineString(Vec<LineStringCoordinates>),
    /// An array of linear ring coordinate arrays.
    Polygon(PolygonRings),
    /// An array of Polygon coordinate arrays.
    MultiPolygon(Vec<PolygonRings>),
}

impl GeometryForm {
    /// The `GeoJSON` type of this geometry.
    #[must_use]
    pub const fn geometry_type(&self) -> GeometryType {
        match self {
            GeometryForm::Point(_) => GeometryType::Point,
            GeometryForm::MultiPoint(_) => GeometryType::MultiPoint,
            GeometryForm::LineString(_) => GeometryType::LineString,
            GeometryForm::MultiLineString(_) => GeometryType::MultiLineString,
            GeometryForm::Polygon(_) => GeometryType::Polygon,
            GeometryForm::MultiPolygon(_) => GeometryType::MultiPolygon,
        }
    }

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

    /// How many positions this geometry holds, every ring and member counted.
    #[must_use]
    pub fn position_count(&self) -> PositionCount {
        let mut census = PositionCensus::default();
        self.take_census(&mut census);
        PositionCount::new(census.positions)
    }

    /// Fold every position of this geometry into the running `census`.
    fn take_census(&self, census: &mut PositionCensus) {
        match self {
            GeometryForm::Point(position) => census.record(position),
            GeometryForm::MultiPoint(positions) => positions.iter().for_each(|position| census.record(position)),
            GeometryForm::LineString(line_string) => line_string.positions().iter().for_each(|position| census.record(position)),
            GeometryForm::MultiLineString(line_strings) => line_strings
                .iter()
                .flat_map(LineStringCoordinates::positions)
                .for_each(|position| census.record(position)),
            GeometryForm::Polygon(rings) => rings.positions().for_each(|position| census.record(position)),
            GeometryForm::MultiPolygon(polygons) => polygons.iter().flat_map(PolygonRings::positions).for_each(|position| census.record(position)),
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
    use crate::{
        dimension::GeometryDimension,
        form::{GeometryForm, PolygonRings},
        line_string_coordinates::LineStringCoordinates,
        linear_ring::LinearRing,
        position::Position,
    };

    fn flat(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn tall(longitude: f64, latitude: f64, altitude: f64) -> Position {
        Position::try_new(longitude, latitude, Some(altitude)).expect("a test position lies on the CRS84 axes")
    }

    fn ring(positions: Vec<Position>) -> LinearRing {
        LinearRing::try_new(positions).expect("a test ring is closed and long enough")
    }

    #[test]
    fn a_geometry_whose_positions_all_carry_an_altitude_is_three_dimensional() {
        let form = GeometryForm::Polygon(PolygonRings::new(vec![ring(vec![
            tall(0.0, 0.0, 5.0),
            tall(1.0, 0.0, 5.0),
            tall(1.0, 1.0, 5.0),
            tall(0.0, 0.0, 5.0),
        ])]));
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::ThreeD);
    }

    #[test]
    fn a_geometry_with_mixed_position_arity_is_two_dimensional() {
        let line = LineStringCoordinates::try_new(vec![flat(0.0, 0.0), tall(1.0, 1.0, 50.0)]).expect("two positions trace a line");
        pretty_assertions::assert_eq!(GeometryForm::LineString(line).dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_geometry_with_no_positions_is_two_dimensional() {
        let form = GeometryForm::MultiPoint(Vec::new());
        pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_multi_polygon_is_three_dimensional_only_when_every_ring_vertex_is() {
        let elevated = GeometryForm::MultiPolygon(vec![PolygonRings::new(vec![ring(vec![
            tall(0.0, 0.0, 1.0),
            tall(1.0, 0.0, 1.0),
            tall(1.0, 1.0, 1.0),
            tall(0.0, 0.0, 1.0),
        ])])]);
        let mixed = GeometryForm::MultiPolygon(vec![PolygonRings::new(vec![ring(vec![
            tall(0.0, 0.0, 1.0),
            flat(1.0, 0.0),
            tall(1.0, 1.0, 1.0),
            tall(0.0, 0.0, 1.0),
        ])])]);
        pretty_assertions::assert_eq!(elevated.dimension(), GeometryDimension::ThreeD);
        pretty_assertions::assert_eq!(mixed.dimension(), GeometryDimension::TwoD);
    }
}
