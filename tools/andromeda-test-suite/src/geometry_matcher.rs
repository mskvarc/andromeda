//! Matchers on a `GeoJSON` geometry a broker answers (RFC 7946 section 3.1), read through the typed model: its type, and
//! the number or the length of the array at a position of its `coordinates`, as a `JSONPath` of indices into them
//! reads it. Numbers compare by [`Equivalent`], so a spelling the specification reads the same matches.

use crate::equivalence::Equivalent;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{
    Number,
    Value,
    geometry::{Geometry, Polygon, Position, Shape},
};

/// One node of a geometry's `coordinates`: an array of nodes, or an ordinate.
#[derive(Debug)]
enum Coordinate<'g> {
    /// A position, or an array of positions, rings, lines or polygons.
    Array(Vec<Coordinate<'g>>),
    /// One ordinate of a position.
    Ordinate(&'g Number),
}

impl<'g> Coordinate<'g> {
    /// The node at `path`, an index into each array in turn.
    fn at(&self, path: &[usize]) -> Option<&Coordinate<'g>> {
        match path.split_first() {
            None => Some(self),
            Some((index, rest)) => match self {
                Coordinate::Array(items) => items.get(*index).and_then(|item| item.at(rest)),
                Coordinate::Ordinate(_) => None,
            },
        }
    }
}

/// The ordinates of `position`, its altitude last when it has one.
fn position(position: &Position) -> Coordinate<'_> {
    let ordinates = [Some(position.longitude()), Some(position.latitude()), position.altitude()];
    Coordinate::Array(ordinates.into_iter().flatten().map(Coordinate::Ordinate).collect())
}

/// The array of `positions`.
fn positions(positions: &[Position]) -> Coordinate<'_> {
    Coordinate::Array(positions.iter().map(position).collect())
}

/// The array of the rings of `polygon`.
fn polygon(polygon: &Polygon) -> Coordinate<'_> {
    Coordinate::Array(polygon.rings().iter().map(|ring| positions(ring.positions())).collect())
}

/// The `coordinates` member of `shape`.
fn coordinates(shape: &Shape) -> Coordinate<'_> {
    match shape {
        Shape::Point(point) => position(point),
        Shape::MultiPoint(points) => positions(points),
        Shape::LineString(line) => positions(line.positions()),
        Shape::MultiLineString(lines) => Coordinate::Array(lines.iter().map(|line| positions(line.positions())).collect()),
        Shape::Polygon(shape) => polygon(shape),
        Shape::MultiPolygon(polygons) => Coordinate::Array(polygons.iter().map(polygon).collect()),
    }
}

/// The `type` member of `shape`.
const fn type_name(shape: &Shape) -> &'static str {
    match shape {
        Shape::Point(_) => "Point",
        Shape::MultiPoint(_) => "MultiPoint",
        Shape::LineString(_) => "LineString",
        Shape::MultiLineString(_) => "MultiLineString",
        Shape::Polygon(_) => "Polygon",
        Shape::MultiPolygon(_) => "MultiPolygon",
    }
}

/// One requirement on a geometry.
#[derive(Debug, Clone)]
enum Check {
    /// The geometry type, as its `type` member names it.
    Type(&'static str),
    /// The ordinate at a path of indices into `coordinates`.
    Ordinate(Vec<usize>, Value),
    /// The length of the array at a path of indices into `coordinates`.
    Length(Vec<usize>, usize),
}

/// The requirements on one geometry.
#[derive(Debug, Clone, Default, MatcherBase)]
pub struct GeometryMatcher {
    /// The requirements, all of which hold.
    checks: Vec<Check>,
}

/// A geometry of any type.
#[must_use]
pub fn geometry() -> GeometryMatcher {
    GeometryMatcher::default()
}

impl GeometryMatcher {
    /// Requires the geometry type to be `name` (`Point`, `LineString`, `Polygon` and their `Multi` forms).
    #[must_use]
    pub fn of_type(mut self, name: &'static str) -> GeometryMatcher {
        self.checks.push(Check::Type(name));
        self
    }

    /// Requires the ordinate at `path`, an index into each array of `coordinates` in turn, to be `value`.
    #[must_use]
    pub fn ordinate<const N: usize>(mut self, path: [usize; N], value: impl Into<Value>) -> GeometryMatcher {
        self.checks.push(Check::Ordinate(path.to_vec(), value.into()));
        self
    }

    /// Requires the array at `path` of `coordinates` (empty for `coordinates` itself) to hold `length` items.
    #[must_use]
    pub fn length<const N: usize>(mut self, path: [usize; N], length: usize) -> GeometryMatcher {
        self.checks.push(Check::Length(path.to_vec(), length));
        self
    }

    /// Why `actual` breaks a requirement, if it does.
    fn mismatch(&self, actual: &Geometry) -> Option<String> {
        let shape = actual.shape();
        let tree = coordinates(shape);
        self.checks
            .iter()
            .find(|check| match check {
                Check::Type(name) => type_name(shape) != *name,
                Check::Ordinate(path, expected) => !matches!(
                    tree.at(path),
                    Some(Coordinate::Ordinate(number)) if Value::Number((*number).clone()).equivalent(expected)
                ),
                Check::Length(path, length) => !matches!(tree.at(path), Some(Coordinate::Array(items)) if items.len() == *length),
            })
            .map(|check| format!("breaks {check:?}: its coordinates are {tree:?}"))
    }
}

impl Matcher<&Geometry> for GeometryMatcher {
    fn matches(&self, actual: &Geometry) -> MatcherResult {
        self.mismatch(actual).is_none().into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("meets {self:?}").into(),
            MatcherResult::NoMatch => format!("doesn't meet {self:?}").into(),
        }
    }

    fn explain_match(&self, actual: &Geometry) -> Description {
        self.mismatch(actual).unwrap_or_else(|| "which meets it".to_owned()).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry_matcher::geometry;
    use googletest::prelude::*;
    use ngsi_ld::model::{
        Value,
        geometry::{Geometry, LineString, LinearRing, Polygon, Position, Shape},
    };

    #[gtest]
    fn a_point_is_matched_by_its_type_and_ordinates() -> Result<()> {
        let point = Geometry::from(Shape::Point(Position::try_with_altitude(4.9041, 52.3676, 10.0)?));
        expect_that!(
            &point,
            geometry().of_type("Point").length([], 3).ordinate([0], Value::Number("4.9041".parse()?))
        );
        expect_that!(&point, geometry().ordinate([2], 10));
        expect_that!(&point, not(geometry().of_type("Polygon")));
        verify_that!(&point, not(geometry().ordinate([0, 1], 4)))
    }

    #[gtest]
    fn a_polygon_is_read_ring_by_ring_and_position_by_position() -> Result<()> {
        let ring = LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?;
        let square = Geometry::from(Shape::Polygon(Polygon::try_new(vec![ring])?));
        expect_that!(&square, geometry().of_type("Polygon").length([], 1).length([0], 4).length([0, 2], 2));
        expect_that!(&square, geometry().ordinate([0, 2, 1], 1));
        verify_that!(&square, not(geometry().length([0, 5], 2)))
    }

    #[gtest]
    fn a_line_string_is_an_array_of_positions() -> Result<()> {
        let line = Geometry::from(Shape::LineString(LineString::try_new(vec![
            Position::try_new(2.0, 48.0)?,
            Position::try_new(3.0, 49.0)?,
        ])?));
        expect_that!(&line, geometry().of_type("LineString").length([], 2).ordinate([1, 1], 49));
        verify_that!(&line, not(geometry().of_type("MultiLineString")))
    }
}
