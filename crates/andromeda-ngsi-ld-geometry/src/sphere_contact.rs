//! Whether two geometries share a point when their edges are read as
//! great-circle arcs and their polygons as [`crate::spherical_polygon`] reads
//! them: the first question `near` asks of two geometries, since two that share
//! a point are zero apart (see [`crate::proximity`]).
//!
//! They share a point exactly when an arc of one meets an arc of the other
//! ([`crate::arc_meeting`]), or, when no arcs meet, when one lies inside a
//! polygon of the other. With no arcs meeting, each connected part of a
//! geometry (a point, a line string, a polygon) lies wholly inside or wholly
//! outside each polygon of the other, so its first position decides.

use crate::{
    arc_index::{IndexedArc, aabb},
    arc_meeting::arcs_meet,
    great_circle::{arc_bounds, arcs},
    spherical_polygon::{SphericalPolygon, polygons_of},
};
use geo::{Coord, Rect};
use rstar::RTree;

/// How far, in degrees, an arc's rectangle is widened before the other
/// geometry's arcs are looked up under it, so that rounding in the rectangle
/// never hides an arc touching it at its edge.
const LOOKUP_MARGIN_DEGREES: f64 = 1e-9;

/// Whether `subject` and `reference` share a point, `reference_arcs` holding
/// the arcs of `reference`.
pub(crate) fn share_a_point(subject: &geo::Geometry<f64>, reference: &geo::Geometry<f64>, reference_arcs: &RTree<IndexedArc>) -> bool {
    arcs(subject).into_iter().any(|arc| {
        reference_arcs
            .locate_in_envelope_intersecting(aabb(&widened(arc_bounds(arc))))
            .any(|indexed| arcs_meet(arc, indexed.arc))
    }) || lies_inside(subject, reference)
        || lies_inside(reference, subject)
}

/// Whether the first position of a connected part of `parts` lies inside a
/// polygon of `container`.
fn lies_inside(parts: &geo::Geometry<f64>, container: &geo::Geometry<f64>) -> bool {
    let polygons: Vec<SphericalPolygon<'_>> = polygons_of(container).into_iter().map(SphericalPolygon::of).collect();
    !polygons.is_empty()
        && first_positions(parts)
            .into_iter()
            .any(|position| polygons.iter().any(|polygon| polygon.covers(position)))
}

/// The first position of every connected part of a geometry.
fn first_positions(geometry: &geo::Geometry<f64>) -> Vec<Coord<f64>> {
    let mut collected = Vec::new();
    collect_first_positions(geometry, &mut collected);
    collected
}

/// Append the first position of every connected part of `geometry` to
/// `collected`; a part with no positions has none.
fn collect_first_positions(geometry: &geo::Geometry<f64>, collected: &mut Vec<Coord<f64>>) {
    match geometry {
        geo::Geometry::Point(point) => collected.push(point.0),
        geo::Geometry::MultiPoint(points) => collected.extend(points.iter().map(|point| point.0)),
        geo::Geometry::Line(line) => collected.push(line.start),
        geo::Geometry::LineString(line_string) => collected.extend(line_string.0.first().copied()),
        geo::Geometry::MultiLineString(line_strings) => collected.extend(line_strings.iter().filter_map(|line_string| line_string.0.first().copied())),
        geo::Geometry::Polygon(polygon) => collected.extend(polygon.exterior().0.first().copied()),
        geo::Geometry::MultiPolygon(polygons) => collected.extend(polygons.iter().filter_map(|polygon| polygon.exterior().0.first().copied())),
        geo::Geometry::Rect(rect) => collected.push(rect.min()),
        geo::Geometry::Triangle(triangle) => collected.push(triangle.v1()),
        geo::Geometry::GeometryCollection(members) => members.iter().for_each(|member| collect_first_positions(member, collected)),
    }
}

/// `rect` widened by [`LOOKUP_MARGIN_DEGREES`] on every side.
fn widened(rect: Rect<f64>) -> Rect<f64> {
    Rect::new(
        Coord {
            x: rect.min().x - LOOKUP_MARGIN_DEGREES,
            y: rect.min().y - LOOKUP_MARGIN_DEGREES,
        },
        Coord {
            x: rect.max().x + LOOKUP_MARGIN_DEGREES,
            y: rect.max().y + LOOKUP_MARGIN_DEGREES,
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::{arc_index::arc_tree, sphere_contact::share_a_point};
    use geo::{Geometry, LineString, MultiPoint, MultiPolygon, Point, Polygon};

    fn ring(positions: &[(f64, f64)]) -> Polygon<f64> {
        Polygon::new(LineString::from(positions.to_vec()), Vec::new())
    }

    fn square(west: f64, south: f64, east: f64, north: f64) -> Geometry<f64> {
        Geometry::Polygon(ring(&[(west, south), (east, south), (east, north), (west, north), (west, south)]))
    }

    fn antimeridian_square() -> Geometry<f64> {
        Geometry::Polygon(ring(&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]))
    }

    /// Whether the two share a point, asserted to be the same either way round.
    fn touch(first: &Geometry<f64>, second: &Geometry<f64>) -> bool {
        let forward = share_a_point(first, second, &arc_tree(second));
        pretty_assertions::assert_eq!(forward, share_a_point(second, first, &arc_tree(first)), "sharing a point is symmetric");
        forward
    }

    #[test]
    fn a_point_inside_a_polygon_across_the_antimeridian_touches_it() {
        assert!(touch(&antimeridian_square(), &Geometry::Point(Point::new(180.0, 0.0))));
        assert!(!touch(&antimeridian_square(), &Geometry::Point(Point::new(0.0, 0.0))));
    }

    #[test]
    fn a_square_whose_corner_is_antipodal_to_a_corner_of_the_other_does_not_touch_it() {
        assert!(!touch(&antimeridian_square(), &square(0.0, 0.0, 10.0, 10.0)));
        assert!(!touch(&square(-10.0, -10.0, 10.0, 10.0), &square(170.0, 0.0, 180.0, 10.0)));
    }

    #[test]
    fn a_polygon_inside_another_touches_it() {
        assert!(touch(&square(-10.0, -10.0, 10.0, 10.0), &square(0.0, 0.0, 5.0, 5.0)));
        assert!(touch(&antimeridian_square(), &square(172.0, -5.0, 178.0, 5.0)));
    }

    #[test]
    fn polygons_whose_edges_cross_touch() {
        assert!(touch(&square(0.0, 0.0, 10.0, 10.0), &square(5.0, 5.0, 15.0, 15.0)));
    }

    #[test]
    fn a_polygon_inside_a_hole_does_not_touch_the_holed_polygon() {
        let holed = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
            vec![LineString::from(vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0), (2.0, 2.0)])],
        ));
        assert!(!touch(&holed, &square(4.0, 4.0, 6.0, 6.0)));
    }

    #[test]
    fn a_point_in_the_band_a_long_edge_bows_over_touches_the_polygon() {
        let area = square(-30.0, 50.0, 30.0, 60.0);
        assert!(touch(&area, &Geometry::Point(Point::new(0.0, 63.0))));
        assert!(!touch(&area, &Geometry::Point(Point::new(0.0, 64.0))));
    }

    #[test]
    fn a_multi_point_touches_a_polygon_holding_any_of_its_points() {
        let points = Geometry::MultiPoint(MultiPoint::from(vec![(0.0, 0.0), (175.0, 5.0)]));
        assert!(touch(&antimeridian_square(), &points));
    }

    #[test]
    fn a_member_of_a_multi_polygon_touches_what_it_holds() {
        let pair = Geometry::MultiPolygon(MultiPolygon::new(vec![
            ring(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]),
            ring(&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]),
        ]));
        assert!(touch(&pair, &Geometry::Point(Point::new(-175.0, 0.0))));
    }

    #[test]
    fn lines_with_antipodal_vertices_do_not_touch() {
        let first = Geometry::LineString(LineString::from(vec![(-10.0, -10.0), (10.0, -10.0)]));
        let second = Geometry::LineString(LineString::from(vec![(170.0, 10.0), (180.0, 10.0)]));
        assert!(!touch(&first, &second));
        let along_the_meridian = Geometry::LineString(LineString::from(vec![(0.0, 0.0), (0.0, 10.0), (100.0, 10.0)]));
        let along_the_equator = Geometry::LineString(LineString::from(vec![(170.0, 0.0), (-170.0, 0.0), (-90.0, 0.0)]));
        assert!(!touch(&along_the_meridian, &along_the_equator));
    }
}
