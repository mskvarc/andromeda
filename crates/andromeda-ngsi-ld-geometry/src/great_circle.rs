//! A geometry read with great-circle edges.
//!
//! `near` measures along great circles, as `PostGIS` `geography` does (see
//! [`crate::proximity`]), and a great-circle arc between two positions does not
//! stay inside the longitude/latitude rectangle of its two ends. Away from the
//! equator it bows towards the nearer pole: an edge spanning 60 degrees of
//! longitude along the 60th parallel reaches 63.43 degrees north. And an edge
//! whose ends lie more than 180 degrees of longitude apart takes the short way
//! round, across the antimeridian, where the planar rectangle of its ends does
//! not reach at all. A box a `near` prefilter starts from is therefore built
//! from [`great_circle_bounds`], never from the planar bounding rectangle,
//! which bounds the vertices alone.
//!
//! The six topological relations of clause 4.10 stay planar, as the `geometry`
//! columns they are rendered against are; this reading is `near`'s alone.

use crate::planar::PlanarGeometry;
use geo::{BoundingRect, Coord, Line, LineString, Polygon, Rect};

/// The longitude/latitude rectangle covering every point of `geometry` when its
/// edges are read as great-circle arcs, or `None` for a geometry with no
/// coordinates.
///
/// It contains the planar bounding rectangle, and widens it only where an edge
/// bows past its ends or crosses the antimeridian.
#[must_use]
pub fn great_circle_bounds(geometry: &PlanarGeometry) -> Option<Rect<f64>> {
    arcs(geometry.as_geo()).into_iter().map(arc_bounds).reduce(|bounds, arc| {
        Rect::new(
            Coord {
                x: bounds.min().x.min(arc.min().x),
                y: bounds.min().y.min(arc.min().y),
            },
            Coord {
                x: bounds.max().x.max(arc.max().x),
                y: bounds.max().y.max(arc.max().y),
            },
        )
    })
}

/// Every edge of a geometry as a great-circle arc, and every isolated position
/// as an arc from the position to itself.
///
/// Together the arcs cover every boundary point and every vertex of the
/// geometry, which is all a minimum distance between two geometries that do
/// not intersect is ever attained on.
pub(crate) fn arcs(geometry: &geo::Geometry<f64>) -> Vec<Line<f64>> {
    let mut collected = Vec::new();
    collect_arcs(geometry, &mut collected);
    collected
}

/// Append the arcs of `geometry` to `collected`.
fn collect_arcs(geometry: &geo::Geometry<f64>, collected: &mut Vec<Line<f64>>) {
    match geometry {
        geo::Geometry::Point(point) => collected.push(Line::new(point.0, point.0)),
        geo::Geometry::MultiPoint(points) => collected.extend(points.iter().map(|point| Line::new(point.0, point.0))),
        geo::Geometry::Line(line) => collected.push(*line),
        geo::Geometry::LineString(line_string) => collect_line_string(line_string, collected),
        geo::Geometry::MultiLineString(line_strings) => line_strings.iter().for_each(|line_string| collect_line_string(line_string, collected)),
        geo::Geometry::Polygon(polygon) => collect_polygon(polygon, collected),
        geo::Geometry::MultiPolygon(polygons) => polygons.iter().for_each(|polygon| collect_polygon(polygon, collected)),
        geo::Geometry::Rect(rect) => collected.extend(rect.to_lines()),
        geo::Geometry::Triangle(triangle) => collected.extend(triangle.to_lines()),
        geo::Geometry::GeometryCollection(members) => members.iter().for_each(|member| collect_arcs(member, collected)),
    }
}

/// Append the arcs of one line string, or of a ring.
///
/// A run of one position has no edge but still marks a point, so it becomes
/// the arc from that position to itself rather than vanishing.
fn collect_line_string(line_string: &LineString<f64>, collected: &mut Vec<Line<f64>>) {
    match line_string.0.as_slice() {
        [only] => collected.push(Line::new(*only, *only)),
        positions => collected.extend(positions.iter().zip(positions.iter().skip(1)).map(|(start, end)| Line::new(*start, *end))),
    }
}

/// Append the arcs of every ring of one polygon.
fn collect_polygon(polygon: &Polygon<f64>, collected: &mut Vec<Line<f64>>) {
    collect_line_string(polygon.exterior(), collected);
    polygon.interiors().iter().for_each(|ring| collect_line_string(ring, collected));
}

/// The longitude/latitude rectangle covering every point of one minor
/// great-circle arc.
pub(crate) fn arc_bounds(arc: Line<f64>) -> Rect<f64> {
    let ends = arc.bounding_rect();
    // A minor arc runs monotonically in longitude between its ends unless its
    // ends lie more than half the globe apart in longitude, in which case it
    // crosses the antimeridian and covers the two outer stretches the planar
    // rectangle leaves out. Only the full range covers both.
    let (west, east) = if (arc.end.x - arc.start.x).abs() > 180.0 {
        (-180.0, 180.0)
    } else {
        (ends.min().x, ends.max().x)
    };
    let (south, north) = latitude_extremes(arc).map_or((ends.min().y, ends.max().y), |(south, north)| {
        (south.min(ends.min().y), north.max(ends.max().y))
    });
    Rect::new(Coord { x: west, y: south }, Coord { x: east, y: north })
}

/// The lowest and highest latitude the arc reaches between its ends, in
/// degrees, or `None` when it reaches neither its great circle's northernmost
/// nor its southernmost point.
///
/// On the unit sphere the arc from `a` to `b` lies on the great circle with
/// normal `n = a x b`. The circle's northernmost point is the north pole's
/// direction with its component along `n` removed, and the arc passes through
/// it exactly when it lies between the two ends in the arc's own sense of
/// rotation, `(a x p) . n >= 0` and `(p x b) . n >= 0`. The southernmost point
/// is its antipode.
fn latitude_extremes(arc: Line<f64>) -> Option<(f64, f64)> {
    let start = unit_vector(arc.start);
    let end = unit_vector(arc.end);
    let normal = cross(start, end);
    let normal_length_squared = dot(normal, normal);
    // Coincident or antipodal ends name no single great circle; the arc is its
    // ends.
    if normal_length_squared < f64::EPSILON {
        return None;
    }
    let along_normal = normal[2] / normal_length_squared;
    let northward = [-along_normal * normal[0], -along_normal * normal[1], 1.0 - along_normal * normal[2]];
    let northward_length = dot(northward, northward).sqrt();
    // The equator has no northernmost point: every point of it is one.
    if northward_length < f64::EPSILON {
        return None;
    }
    let northmost = northward.map(|component| component / northward_length);
    let southmost = northmost.map(|component| -component);
    let on_arc = |point: [f64; 3]| dot(cross(start, point), normal) >= 0.0 && dot(cross(point, end), normal) >= 0.0;
    let north = on_arc(northmost).then(|| latitude_of(northmost));
    let south = on_arc(southmost).then(|| latitude_of(southmost));
    match (south, north) {
        (None, None) => None,
        (south, north) => Some((south.unwrap_or(f64::INFINITY), north.unwrap_or(f64::NEG_INFINITY))),
    }
}

/// The unit vector of a longitude/latitude position on the sphere.
fn unit_vector(position: Coord<f64>) -> [f64; 3] {
    let (longitude, latitude) = (position.x.to_radians(), position.y.to_radians());
    [latitude.cos() * longitude.cos(), latitude.cos() * longitude.sin(), latitude.sin()]
}

/// The latitude, in degrees, of a unit vector.
fn latitude_of(vector: [f64; 3]) -> f64 {
    vector[2].clamp(-1.0, 1.0).asin().to_degrees()
}

/// The cross product of two vectors.
const fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The dot product of two vectors.
const fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        form::{GeometryForm, PolygonRings},
        geometry::Geometry,
        great_circle::great_circle_bounds,
        line_string_coordinates::LineStringCoordinates,
        linear_ring::LinearRing,
        position::Position,
    };
    use geo::{Haversine, InterpolatePoint, Point, Rect};

    fn position(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn line(positions: &[(f64, f64)]) -> Geometry {
        Geometry::new(GeometryForm::LineString(
            LineStringCoordinates::try_new(positions.iter().map(|&(longitude, latitude)| position(longitude, latitude)).collect())
                .expect("a test line has two positions or more"),
        ))
    }

    fn bounds(geometry: &Geometry) -> Rect<f64> {
        great_circle_bounds(&geometry.planar()).expect("a test geometry has coordinates")
    }

    /// Whether every point sampled along the great-circle edge from `start` to
    /// `end` lies inside `rect`.
    fn covers_the_arc(rect: Rect<f64>, start: (f64, f64), end: (f64, f64)) -> bool {
        (0..=1000).all(|step| {
            let point = Haversine.point_at_ratio_between(Point::new(start.0, start.1), Point::new(end.0, end.1), f64::from(step) / 1000.0);
            rect.min().x - 1e-9 <= point.x() && point.x() <= rect.max().x + 1e-9 && rect.min().y - 1e-9 <= point.y() && point.y() <= rect.max().y + 1e-9
        })
    }

    #[test]
    fn a_long_edge_along_a_northern_parallel_bows_towards_the_pole() {
        // The great circle through (-30, 60) and (30, 60) peaks at
        // atan(tan 60 / cos 30) = 63.435 degrees north, which `PostGIS`
        // `geography` measures a point at (0, 63.43) as 551.64 m from.
        let rect = bounds(&line(&[(-30.0, 60.0), (30.0, 60.0)]));
        assert!((rect.max().y - 63.434_948_8).abs() < 1e-6, "north edge {}", rect.max().y);
        assert!((rect.min().y - 60.0).abs() < 1e-12, "south edge {}", rect.min().y);
        assert!(covers_the_arc(rect, (-30.0, 60.0), (30.0, 60.0)));
    }

    #[test]
    fn a_long_edge_along_a_southern_parallel_bows_towards_the_south_pole() {
        let rect = bounds(&line(&[(-30.0, -60.0), (30.0, -60.0)]));
        assert!((rect.min().y + 63.434_948_8).abs() < 1e-6, "south edge {}", rect.min().y);
        assert!((rect.max().y + 60.0).abs() < 1e-12, "north edge {}", rect.max().y);
    }

    #[test]
    fn an_edge_crossing_the_antimeridian_spans_every_longitude() {
        let rect = bounds(&line(&[(179.9, 0.0), (-179.9, 0.0)]));
        pretty_assertions::assert_eq!((rect.min().x, rect.max().x), (-180.0, 180.0));
        assert!(covers_the_arc(rect, (179.9, 0.0), (-179.9, 0.0)));
    }

    #[test]
    fn a_meridian_and_the_equator_stay_inside_the_rectangle_of_their_ends() {
        pretty_assertions::assert_eq!(bounds(&line(&[(10.0, 0.0), (10.0, 50.0)])), Rect::new((10.0, 0.0), (10.0, 50.0)));
        pretty_assertions::assert_eq!(bounds(&line(&[(-30.0, 0.0), (30.0, 0.0)])), Rect::new((-30.0, 0.0), (30.0, 0.0)));
    }

    #[test]
    fn a_building_sized_polygon_bows_by_a_negligible_amount() {
        let ring = LinearRing::try_new(vec![
            position(8.808_693, 53.075_171),
            position(8.810_783, 53.075_171),
            position(8.810_783, 53.076_429),
            position(8.808_693, 53.076_429),
            position(8.808_693, 53.075_171),
        ])
        .expect("a test square is a closed ring");
        let square = Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![ring])));
        let rect = bounds(&square);
        let planar = square.planar().bounding_rect().expect("a square has a bounding rectangle");
        assert!(rect.max().y - planar.max().y < 1e-7, "a 140 m edge bows by under a centimetre");
        pretty_assertions::assert_eq!((rect.min(), (rect.max().x)), (planar.min(), planar.max().x));
    }

    #[test]
    fn every_sampled_point_of_assorted_arcs_lies_inside_their_bounds() {
        let edges = [
            ((-120.0, 45.0), (100.0, 20.0)),
            ((0.0, 80.0), (179.0, 80.0)),
            ((-170.0, -40.0), (170.0, -60.0)),
            ((10.0, -10.0), (60.0, 10.0)),
            ((-45.0, 70.0), (45.0, 70.0)),
        ];
        for (start, end) in edges {
            let rect = bounds(&line(&[start, end]));
            assert!(covers_the_arc(rect, start, end), "{start:?} to {end:?} escapes {rect:?}");
        }
    }

    #[test]
    fn a_geometry_with_no_coordinates_has_no_bounds() {
        assert!(great_circle_bounds(&Geometry::new(GeometryForm::Polygon(PolygonRings::new(Vec::new()))).planar()).is_none());
        assert!(great_circle_bounds(&Geometry::new(GeometryForm::MultiPoint(Vec::new())).planar()).is_none());
    }

    #[test]
    fn a_point_is_its_own_bounds() {
        let point = Geometry::new(GeometryForm::Point(position(8.8072, 53.0758)));
        pretty_assertions::assert_eq!(bounds(&point), Rect::new((8.8072, 53.0758), (8.8072, 53.0758)));
    }
}
