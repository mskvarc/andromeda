//! The two operands of a topological relation noded against each other, so
//! that the intersection graph a relation is computed over is the same graph
//! whichever operand a crossing is found from.
//!
//! `geo` computes an intersection matrix over a graph that splits each
//! geometry at its own crossings and at its crossings with the other, and it
//! computes the point of each crossing once for every pair of segments that
//! meet there. Where one operand crosses a segment both operands share, that
//! crossing is computed once against each copy of the segment, the two
//! results can round apart, and the shared segment is split at two different
//! points: the graph then holds no edge common to both operands, and two lines
//! sharing a segment meet in points alone. A `LineString` that runs along the
//! reference and crosses back over itself on the shared part is such a pair,
//! and clause 4.10 `overlaps`, the Simple Features relation, holds for it.
//!
//! Noding the pair first makes every proper crossing a vertex of each segment
//! it lies on, computed once from the two segments in one order whichever
//! operand, path or direction they come in. Copies of one segment then split at
//! the same vertex, and the graph meets every crossing at a vertex it already
//! holds instead of computing it again.

use crate::relation_reading::Operand;
use geo::{
    Coord,
    Geometry,
    Line,
    LineString,
    MultiLineString,
    MultiPolygon,
    Polygon,
    algorithm::sweep::{Cross, Intersections},
    line_intersection::{LineIntersection, line_intersection},
};
use std::{borrow::Cow, cmp::Ordering, collections::BTreeMap, iter::once};

/// Both operands of a relation, every proper crossing between two of their
/// segments a vertex of each segment it lies on.
///
/// An operand no crossing lies on is borrowed as it is. So is the whole pair
/// when either operand has no segment: the graph places a point against the
/// other operand by orientation tests over the written coordinates, and only
/// two segments make it compute a crossing.
#[derive(Debug)]
pub(crate) struct NodedPair<'a> {
    /// The subject, noded.
    pub(crate) subject: Cow<'a, Geometry<f64>>,
    /// The reference, noded.
    pub(crate) reference: Cow<'a, Geometry<f64>>,
}

impl<'a> NodedPair<'a> {
    /// Node `subject` and `reference` against each other.
    pub(crate) fn of(subject: &'a Geometry<f64>, reference: &'a Geometry<f64>) -> NodedPair<'a> {
        let (subject_paths, reference_paths) = (paths(subject), paths(reference));
        let subject_segments = segments(Operand::Subject, &subject_paths);
        let reference_segments = segments(Operand::Reference, &reference_paths);
        if subject_segments.is_empty() || reference_segments.is_empty() {
            return NodedPair {
                subject: Cow::Borrowed(subject),
                reference: Cow::Borrowed(reference),
            };
        }
        let mut subject_splits = Splits::default();
        let mut reference_splits = Splits::default();
        let crossings = Intersections::from_iter(subject_segments.into_iter().chain(reference_segments))
            .filter(|(_, _, meeting)| meeting.is_proper())
            .filter_map(|(one, other, _)| crossing_point(one.line, other.line).map(|point| [(one, point), (other, point)]));
        for (segment, point) in crossings.flatten() {
            match segment.operand {
                Operand::Subject => subject_splits.add(&segment, point),
                Operand::Reference => reference_splits.add(&segment, point),
            }
        }
        NodedPair {
            subject: subject_splits.applied_to(subject),
            reference: reference_splits.applied_to(reference),
        }
    }
}

/// One segment of an operand, and where it sits in it.
#[derive(Debug, Clone, Copy)]
struct Segment {
    /// The operand the segment belongs to.
    operand: Operand,
    /// The path the segment belongs to, in the order [`paths`] lists them.
    path: usize,
    /// The segment's place along its path, from its first vertex.
    index: usize,
    /// The segment.
    line: Line<f64>,
}

// Written by hand: `Cross` is the sweep's own trait, which has no derive.
impl Cross for Segment {
    type Scalar = f64;

    fn line(&self) -> Line<f64> {
        self.line
    }
}

/// The crossings to insert into one operand, by path and by segment.
#[derive(Debug, Default)]
struct Splits(BTreeMap<usize, BTreeMap<usize, Vec<Coord<f64>>>>);

impl Splits {
    /// Record that `point` lies on `segment`.
    fn add(&mut self, segment: &Segment, point: Coord<f64>) {
        self.0.entry(segment.path).or_default().entry(segment.index).or_default().push(point);
    }

    /// `geometry` with every recorded crossing inserted, or `geometry` itself
    /// when none was recorded.
    fn applied_to<'g>(&self, geometry: &'g Geometry<f64>) -> Cow<'g, Geometry<f64>> {
        if self.0.is_empty() {
            return Cow::Borrowed(geometry);
        }
        let mut next = 0;
        let mut noded = |path: &LineString<f64>| {
            let noded = noded_path(path, self.0.get(&next));
            next += 1;
            noded
        };
        match geometry {
            Geometry::LineString(line) => Cow::Owned(Geometry::LineString(noded(line))),
            Geometry::MultiLineString(lines) => Cow::Owned(Geometry::MultiLineString(MultiLineString::new(lines.iter().map(&mut noded).collect()))),
            Geometry::Polygon(polygon) => Cow::Owned(Geometry::Polygon(noded_polygon(polygon, &mut noded))),
            Geometry::MultiPolygon(polygons) => Cow::Owned(Geometry::MultiPolygon(MultiPolygon::new(
                polygons.iter().map(|polygon| noded_polygon(polygon, &mut noded)).collect(),
            ))),
            // No path, so no segment a crossing was recorded on.
            Geometry::Point(_) | Geometry::MultiPoint(_) | Geometry::Line(_) | Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::GeometryCollection(_) => {
                Cow::Borrowed(geometry)
            }
        }
    }
}

/// Every path of a geometry whose segments the noding reads: the line of a
/// `LineString`, the lines of a `MultiLineString` and the rings of every
/// polygon, exterior first.
///
/// A projection writes the six `GeoJSON` geometry forms alone, so the four
/// `geo` geometries with no `GeoJSON` form have no path read.
fn paths(geometry: &Geometry<f64>) -> Vec<&LineString<f64>> {
    match geometry {
        Geometry::LineString(line) => vec![line],
        Geometry::MultiLineString(lines) => lines.iter().collect(),
        Geometry::Polygon(polygon) => rings(polygon).collect(),
        Geometry::MultiPolygon(polygons) => polygons.iter().flat_map(rings).collect(),
        Geometry::Point(_) | Geometry::MultiPoint(_) | Geometry::Line(_) | Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::GeometryCollection(_) => {
            Vec::new()
        }
    }
}

/// The rings of a polygon, exterior first.
fn rings(polygon: &Polygon<f64>) -> impl Iterator<Item = &LineString<f64>> {
    once(polygon.exterior()).chain(polygon.interiors())
}

/// Every segment of an operand's paths.
fn segments(operand: Operand, paths: &[&LineString<f64>]) -> Vec<Segment> {
    paths
        .iter()
        .enumerate()
        .flat_map(|(path, line)| line.lines().enumerate().map(move |(index, line)| Segment { operand, path, index, line }))
        .collect()
}

/// A polygon with every ring noded, in the order [`rings`] lists them.
fn noded_polygon(polygon: &Polygon<f64>, noded: &mut impl FnMut(&LineString<f64>) -> LineString<f64>) -> Polygon<f64> {
    let exterior = noded(polygon.exterior());
    Polygon::new(exterior, polygon.interiors().iter().map(noded).collect())
}

/// A path with the crossings recorded for each of its segments inserted
/// between the segment's ends, nearest its first end first.
///
/// A crossing computed at one of the segment's own ends is that vertex already
/// and is not inserted again, and one recorded twice is inserted once.
fn noded_path(path: &LineString<f64>, splits: Option<&BTreeMap<usize, Vec<Coord<f64>>>>) -> LineString<f64> {
    let Some(splits) = splits else {
        return path.clone();
    };
    let mut vertices = Vec::with_capacity(path.0.len() + splits.values().map(Vec::len).sum::<usize>());
    vertices.extend(path.0.first());
    for (index, line) in path.lines().enumerate() {
        if let Some(points) = splits.get(&index) {
            let mut points: Vec<Coord<f64>> = points.iter().copied().filter(|point| *point != line.start && *point != line.end).collect();
            points.sort_by(|one, other| {
                squared_distance(line.start, *one)
                    .total_cmp(&squared_distance(line.start, *other))
                    .then_with(|| order(*one, *other))
            });
            points.dedup();
            vertices.extend(points);
        }
        vertices.push(line.end);
    }
    LineString(vertices)
}

/// The squared planar distance between two coordinates, which orders points
/// along a segment as their distance does.
fn squared_distance(from: Coord<f64>, to: Coord<f64>) -> f64 {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    dx.mul_add(dx, dy * dy)
}

/// The point where two segments cross properly, computed from the two in one
/// order whatever order and direction they are given in, so that every copy
/// of either segment is split at the same point.
///
/// `None` when they do not cross in a single point, which a pair the sweep
/// found crossing properly never is.
fn crossing_point(one: Line<f64>, other: Line<f64>) -> Option<Coord<f64>> {
    let (one, other) = (directed(one), directed(other));
    let (first, second) = match line_order(one, other) {
        Ordering::Less | Ordering::Equal => (one, other),
        Ordering::Greater => (other, one),
    };
    match line_intersection(first, second)? {
        LineIntersection::SinglePoint { intersection, .. } => Some(intersection),
        LineIntersection::Collinear { .. } => None,
    }
}

/// A segment running from the lesser of its ends to the greater.
fn directed(line: Line<f64>) -> Line<f64> {
    match order(line.start, line.end) {
        Ordering::Less | Ordering::Equal => line,
        Ordering::Greater => Line::new(line.end, line.start),
    }
}

/// A total order over segments, by their first ends and then their second.
fn line_order(one: Line<f64>, other: Line<f64>) -> Ordering {
    order(one.start, other.start).then_with(|| order(one.end, other.end))
}

/// A total order over coordinates, by longitude and then latitude.
fn order(one: Coord<f64>, other: Coord<f64>) -> Ordering {
    one.x.total_cmp(&other.x).then_with(|| one.y.total_cmp(&other.y))
}

#[cfg(test)]
mod tests {
    use crate::noded_pair::{NodedPair, crossing_point};
    use geo::{Geometry, Line, LineString, MultiPolygon, Point, Polygon, Relate, algorithm::relate::IntersectionMatrix, coord};
    use pretty_assertions::assert_eq;
    use std::{borrow::Cow, str::FromStr};

    /// The reference line of the crossing-back fixtures.
    fn reference() -> Geometry<f64> {
        Geometry::LineString(LineString::from(vec![
            (17.041_112_899_780_3, 48.208_068_847_656_3),
            (17.044_538_497_924_8, 48.199_306_488_037_1),
            (17.049_634_933_471_7, 48.196_449_279_785_2),
            (17.052_634_9, 48.198_449_3),
        ]))
    }

    /// A line along the reference's second segment that turns back and
    /// crosses that segment.
    fn crossing_back() -> Geometry<f64> {
        Geometry::LineString(LineString::from(vec![
            (17.044_538_497_924_8, 48.199_306_488_037_1),
            (17.049_634_933_471_7, 48.196_449_279_785_2),
            (17.048_576_354_980_5, 48.197_162_628_173_8),
            (17.046_571_731_567_4, 48.193_210_601_806_6),
        ]))
    }

    fn square(west: f64, south: f64, side: f64) -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![
                (west, south),
                (west + side, south),
                (west + side, south + side),
                (west, south + side),
                (west, south),
            ]),
            Vec::new(),
        )
    }

    /// ETSI GS CIM 009 v1.9.1 clause 4.10 reads `overlaps` by the Simple
    /// Features relation: the two lines share the reference's second segment,
    /// and each runs outside the other, so their interiors meet in a line.
    #[test]
    fn a_line_crossing_back_over_a_shared_segment_relates_with_the_interiors_meeting_in_a_line() {
        let (subject, reference) = (crossing_back(), reference());
        let noded = NodedPair::of(&subject, &reference);
        assert_eq!(noded.subject.relate(noded.reference.as_ref()), matrix("1F10F0102"));
        let reversed = NodedPair::of(&reference, &subject);
        assert_eq!(reversed.subject.relate(reversed.reference.as_ref()), matrix("101FF0102"));
    }

    fn matrix(spelled: &str) -> IntersectionMatrix {
        IntersectionMatrix::from_str(spelled).expect("a DE-9IM matrix")
    }

    #[test]
    fn both_copies_of_a_shared_segment_split_at_one_vertex() {
        let (subject, reference) = (crossing_back(), reference());
        let noded = NodedPair::of(&subject, &reference);
        let (Geometry::LineString(subject), Geometry::LineString(reference)) = (noded.subject.as_ref(), noded.reference.as_ref()) else {
            panic!("noding keeps each LineString a LineString");
        };
        assert_eq!(subject.0.len(), 6, "the crossing splits the shared segment and the segment crossing it");
        assert_eq!(reference.0.len(), 5, "the crossing splits the shared segment alone");
        assert_eq!(subject.0[1], reference.0[2]);
        assert_eq!(subject.0[4], reference.0[2]);
    }

    #[test]
    fn a_crossing_is_computed_alike_whatever_order_and_direction_its_segments_come_in() {
        let one = Line::new(coord! { x: 0.1, y: 0.3 }, coord! { x: 7.7, y: 2.9 });
        let other = Line::new(coord! { x: 1.3, y: 5.1 }, coord! { x: 6.1, y: -3.7 });
        let reversed = |line: Line<f64>| Line::new(line.end, line.start);
        let point = crossing_point(one, other);
        assert!(point.is_some());
        for (first, second) in [(other, one), (reversed(one), other), (other, reversed(one)), (reversed(other), reversed(one))] {
            assert_eq!(crossing_point(first, second), point);
        }
    }

    #[test]
    fn a_pair_with_a_point_operand_is_borrowed_as_it_is() {
        let (point, area) = (Geometry::Point(Point::new(0.5, 0.5)), Geometry::Polygon(square(0.0, 0.0, 1.0)));
        let noded = NodedPair::of(&point, &area);
        assert!(matches!((&noded.subject, &noded.reference), (Cow::Borrowed(_), Cow::Borrowed(_))));
    }

    #[test]
    fn an_operand_no_crossing_lies_on_is_borrowed_as_it_is() {
        let (inner, outer) = (Geometry::Polygon(square(0.25, 0.25, 0.5)), Geometry::Polygon(square(0.0, 0.0, 1.0)));
        let noded = NodedPair::of(&inner, &outer);
        assert!(matches!((&noded.subject, &noded.reference), (Cow::Borrowed(_), Cow::Borrowed(_))));
    }

    /// The crossings land in the ring they were found on: the line crosses
    /// the second polygon's hole and nothing of the first polygon.
    #[test]
    fn a_crossing_is_inserted_into_the_ring_it_lies_on() {
        let holed = Polygon::new(square(10.0, 0.0, 4.0).exterior().clone(), vec![square(11.0, 1.0, 2.0).exterior().clone()]);
        let areas = Geometry::MultiPolygon(MultiPolygon::new(vec![square(0.0, 0.0, 1.0), holed]));
        let line = Geometry::LineString(LineString::from(vec![(12.0, 2.0), (12.0, 3.5)]));
        let noded = NodedPair::of(&areas, &line);
        let Geometry::MultiPolygon(noded) = noded.subject.as_ref() else {
            panic!("noding keeps a MultiPolygon a MultiPolygon");
        };
        assert_eq!(noded.0[0], square(0.0, 0.0, 1.0));
        assert_eq!(noded.0[1].exterior(), square(10.0, 0.0, 4.0).exterior());
        assert_eq!(
            noded.0[1].interiors()[0],
            LineString::from(vec![(11.0, 1.0), (13.0, 1.0), (13.0, 3.0), (12.0, 3.0), (11.0, 3.0), (11.0, 1.0)])
        );
    }
}
