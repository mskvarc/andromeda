//! Whether two geometries come within a distance of each other, the question
//! clause 4.10 `near` asks.
//!
//! Clause 4.10 matches `near;maxDistance` against "the buffer geometric object
//! (as defined by [14])" of the reference, and OGC 06-103r4 clause 6.1.2.4
//! defines both `Buffer` and `Distance` through "the shortest distance between
//! any two Points in the two geometric objects". The separation of two
//! geometries is therefore their minimum distance (zero when they share a
//! point) and never a distance between centroids: a building whose nearest
//! wall is 100 m away is 100 m away, wherever its middle lies.
//!
//! Clause 4.10 gives distances "in meters" and names no earth model. The store
//! answers `near` with `ST_DWithin` and `ST_Distance` over `geography`, which
//! measure on the WGS 84 spheroid, the datum RFC 7946 places every `GeoJSON`
//! position in, and this module measures the same way so that an in-memory
//! verdict and a store verdict agree:
//!
//! 1. Two geometries that intersect are zero apart. Containment is decided on
//!    the planar projection, as the six topological relations are.
//! 2. Otherwise the closest pair is found on the sphere, with every edge read
//!    as a great-circle arc: for two disjoint geometries made of minor arcs the
//!    minimum separation is attained at a vertex of one of them, so every
//!    vertex is measured against the arcs of the other geometry, in both
//!    directions.
//! 3. The distance of a pair is its WGS 84 geodesic, by Karney's algorithm.
//!
//! That is what `PostGIS` does for `geography`: it locates the closest points
//! on the sphere and measures them on the spheroid. A spheroidal geodesic is
//! never more than 0.56 % shorter nor 0.45 % longer than the spherical one
//! between the same two positions, so a pair is decided from its cheap
//! spherical distance whenever that band lies wholly on one side of the
//! threshold, and the geodesic is computed only inside it.
//!
//! Only the arcs near enough to a vertex to matter are measured: each geometry's
//! arcs are held in an R-tree under their great-circle bounds, and a vertex
//! looks only at those inside its [`NearEnvelope`], which shrinks as closer
//! pairs are found. A pair whose longest possible geodesic is already within
//! the distance answers the question, so the scan stops there; only a closest
//! pair inside the half-percent band around the distance is searched for in
//! full. Two geometries of the 100 000 positions the admission budget allows
//! are decided in tens of milliseconds rather than the hours an all-pairs scan
//! would take.
//!
//! Sorting by distance is a different measure: clause 4.23.1 defines it as the
//! spherical Haversine distance, and the store renders it as such.

use crate::{
    great_circle::{arc_bounds, arcs},
    near_envelope::NearEnvelope,
    positive_distance::PositiveDistance,
};
use geo::{Closest, CoordsIter, Distance, Geodesic, Haversine, HaversineClosestPoint, Intersects, Line, Point, Rect};
use rstar::{AABB, RTree, RTreeObject};
use std::cell::OnceCell;

/// The least ratio of a WGS 84 geodesic to the spherical (`geo`'s Haversine,
/// radius 6 371 008.8 m) distance between the same two positions, rounded
/// down.
///
/// At equal latitude and longitude the spheroid's line element
/// `M^2 dlat^2 + N^2 cos^2(lat) dlon^2` and the sphere's
/// `R^2 (dlat^2 + cos^2(lat) dlon^2)` differ by a factor between the least
/// radius of curvature over `R` and the greatest over `R`. The least is the
/// meridional radius at the equator, `b^2 / a = 6 335 439.3` m, so every path,
/// and therefore every geodesic, is at least 0.994 417 times as long on the
/// spheroid.
const SPHEROID_OVER_SPHERE_MIN: f64 = 0.994_41;

/// The greatest ratio of a WGS 84 geodesic to the spherical distance between
/// the same two positions, rounded up: the greatest radius of curvature, at the
/// poles, `a^2 / b = 6 399 593.6` m, over the sphere's radius gives 1.004 487.
const SPHEROID_OVER_SPHERE_MAX: f64 = 1.004_49;

/// Whether a separation of exactly the threshold distance counts as within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// A separation equal to the distance is within it, as `ST_DWithin`
    /// reads `maxDistance`.
    Inclusive,
    /// Only a separation strictly less than the distance is within it, the
    /// complement of `ST_Distance(..) >= minDistance`.
    Exclusive,
}

/// Two geometries whose separation is about to be compared with one or more
/// distances.
///
/// Built once per pair, so a `near` carrying both a `maxDistance` and a
/// `minDistance` intersects the two geometries once and indexes each one's
/// arcs at most once.
#[derive(Debug)]
pub struct Proximity<'a> {
    /// The geometry read off the entity or registration.
    subject: &'a geo::Geometry<f64>,
    /// The geometry the query supplied.
    reference: &'a geo::Geometry<f64>,
    /// Whether the two share a point.
    separation: Separation,
    /// The subject's arcs, indexed on first use.
    subject_arcs: OnceCell<RTree<IndexedArc>>,
    /// The reference's arcs, indexed on first use.
    reference_arcs: OnceCell<RTree<IndexedArc>>,
}

/// Whether two geometries share a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Separation {
    /// They intersect, so they are zero apart.
    Touching,
    /// They share no point.
    Apart,
}

impl<'a> Proximity<'a> {
    /// The pair `subject` and `reference`, or `None` when either has no
    /// coordinates: a geometry that is nowhere is at no distance from anything
    /// (clause 4.10 counts an Entity without the target `GeoProperty` as
    /// non-matching, and an empty geometry conveys no more than that).
    #[must_use]
    pub fn between(subject: &'a geo::Geometry<f64>, reference: &'a geo::Geometry<f64>) -> Option<Proximity<'a>> {
        if subject.coords_count() == 0 || reference.coords_count() == 0 {
            return None;
        }
        let separation = if subject.intersects(reference) {
            Separation::Touching
        } else {
            Separation::Apart
        };
        Some(Proximity {
            subject,
            reference,
            separation,
            subject_arcs: OnceCell::new(),
            reference_arcs: OnceCell::new(),
        })
    }

    /// Whether the two geometries come within `distance` of each other on the
    /// WGS 84 spheroid, the threshold itself counting as `boundary` says.
    #[must_use]
    pub fn comes_within(&self, distance: PositiveDistance, boundary: Boundary) -> bool {
        match self.separation {
            // Zero is within every distance, which is strictly positive.
            Separation::Touching => true,
            Separation::Apart => {
                let mut search = ClosestPairSearch::new(distance.metres(), boundary);
                search.scan(self.subject, self.reference_arcs.get_or_init(|| arc_tree(self.reference)))
                    || search.scan(self.reference, self.subject_arcs.get_or_init(|| arc_tree(self.subject)))
                    || search.settle()
            }
        }
    }
}

/// The search for the pair of points, one on each geometry, that is closest on
/// the sphere, run only as far as a threshold distance needs.
///
/// The verdict is the geodesic of the sphere's closest pair, as in `PostGIS`.
/// A pair whose longest possible geodesic is already within the threshold
/// settles it at once, because the closest pair can only be closer. Short of
/// that, the closest pair is tracked, and only arcs that could still hold a
/// closer one are measured: within the reach a pair inside the threshold could
/// have, and within the reach of the closest pair found so far.
#[derive(Debug)]
struct ClosestPairSearch {
    /// The threshold, in metres.
    distance: f64,
    /// Whether the threshold itself counts as within.
    boundary: Boundary,
    /// The pair closest on the sphere found so far.
    closest: Option<ClosestPair>,
}

/// A vertex of one geometry and the point of the other closest to it.
#[derive(Debug, Clone, Copy)]
struct ClosestPair {
    /// The vertex.
    vertex: Point<f64>,
    /// The closest point of the other geometry's arcs.
    closest: Point<f64>,
    /// Their spherical distance, in metres.
    spherical: f64,
}

impl ClosestPairSearch {
    /// A search against `distance` metres, before any pair is measured.
    const fn new(distance: f64, boundary: Boundary) -> ClosestPairSearch {
        ClosestPairSearch {
            distance,
            boundary,
            closest: None,
        }
    }

    /// Measure every vertex of `from` against the arcs of the other geometry
    /// in `onto`, returning `true` as soon as a pair settles that the
    /// geometries come within the threshold.
    fn scan(&mut self, from: &geo::Geometry<f64>, onto: &RTree<IndexedArc>) -> bool {
        from.coords_iter().any(|vertex| {
            let point = Point::from(vertex);
            NearEnvelope::around_metres(Rect::new(vertex, vertex), self.reach())
                .rects()
                .iter()
                .any(|reach| {
                    onto.locate_in_envelope_intersecting(aabb(reach)).any(|indexed| {
                        let (closest, spherical) = closest_on_arc(indexed.arc, point);
                        if self.closest.is_none_or(|known| spherical < known.spherical) {
                            self.closest = Some(ClosestPair {
                                vertex: point,
                                closest,
                                spherical,
                            });
                        }
                        clearly_within(spherical, self.distance, self.boundary)
                    })
                })
        })
    }

    /// How far, in metres of geodesic, a pair can be and still matter: closer
    /// on the sphere than the closest pair found so far, and close enough on
    /// the sphere that its geodesic could be within the threshold.
    fn reach(&self) -> f64 {
        let within_threshold = self.distance * SPHEROID_OVER_SPHERE_MAX / SPHEROID_OVER_SPHERE_MIN;
        self.closest
            .map_or(within_threshold, |known| within_threshold.min(known.spherical * SPHEROID_OVER_SPHERE_MAX))
    }

    /// The verdict once every vertex is measured and none settled it: whether
    /// the sphere's closest pair is within the threshold on the spheroid.
    /// With no pair within reach the geometries are beyond it.
    fn settle(&self) -> bool {
        self.closest
            .is_some_and(|pair| within(pair.spherical, || Geodesic.distance(pair.vertex, pair.closest), self.distance, self.boundary))
    }
}

/// One great-circle arc of a geometry, under the box that covers it.
#[derive(Debug)]
struct IndexedArc {
    /// The arc.
    arc: Line<f64>,
    /// Its great-circle bounds.
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for IndexedArc {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}

/// Every arc of a geometry, indexed by its great-circle bounds.
fn arc_tree(geometry: &geo::Geometry<f64>) -> RTree<IndexedArc> {
    RTree::bulk_load(
        arcs(geometry)
            .into_iter()
            .map(|arc| IndexedArc {
                arc,
                envelope: aabb(&arc_bounds(arc)),
            })
            .collect(),
    )
}

/// The point of `arc` closest to `vertex` on the sphere, and its spherical
/// distance in metres.
///
/// `geo`'s cross-track computation can yield no point at an exact degeneracy (a
/// rounding `acos` past 1 when the foot of the perpendicular falls on an end);
/// the nearer end of the arc then stands in, which is where such a foot lies.
fn closest_on_arc(arc: Line<f64>, vertex: Point<f64>) -> (Point<f64>, f64) {
    match arc.haversine_closest_point(&vertex) {
        Closest::Intersection(point) => (point, 0.0),
        Closest::SinglePoint(point) => {
            let spherical = Haversine.distance(point, vertex);
            if spherical.is_nan() { nearer_end(arc, vertex) } else { (point, spherical) }
        }
        Closest::Indeterminate => nearer_end(arc, vertex),
    }
}

/// The end of `arc` nearer to `vertex`, and its spherical distance in metres.
fn nearer_end(arc: Line<f64>, vertex: Point<f64>) -> (Point<f64>, f64) {
    let (start, end) = (arc.start_point(), arc.end_point());
    let (to_start, to_end) = (Haversine.distance(start, vertex), Haversine.distance(end, vertex));
    if to_start <= to_end { (start, to_start) } else { (end, to_end) }
}

/// Whether a pair whose spherical distance is `spherical` metres is within
/// `distance` metres on the spheroid whatever its geodesic, so that the closest
/// pair, no farther apart on the sphere, is within it too.
fn clearly_within(spherical: f64, distance: f64, boundary: Boundary) -> bool {
    let longest = spherical * SPHEROID_OVER_SPHERE_MAX;
    match boundary {
        Boundary::Inclusive => longest <= distance,
        Boundary::Exclusive => longest < distance,
    }
}

/// Whether a pair whose spherical distance is `spherical` metres is within
/// `distance` metres on the spheroid, computing the geodesic only when the
/// spherical distance alone cannot tell.
fn within(spherical: f64, geodesic: impl FnOnce() -> f64, distance: f64, boundary: Boundary) -> bool {
    let shortest = spherical * SPHEROID_OVER_SPHERE_MIN;
    let longest = spherical * SPHEROID_OVER_SPHERE_MAX;
    match boundary {
        Boundary::Inclusive => longest <= distance || (shortest <= distance && geodesic() <= distance),
        Boundary::Exclusive => longest < distance || (shortest < distance && geodesic() < distance),
    }
}

/// The R-tree box of a longitude/latitude rectangle.
fn aabb(rect: &Rect<f64>) -> AABB<[f64; 2]> {
    AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        positive_distance::PositiveDistance,
        proximity::{Boundary, Proximity, SPHEROID_OVER_SPHERE_MAX, SPHEROID_OVER_SPHERE_MIN, within},
    };
    use geo::{Distance, Geodesic, Geometry, Haversine, LineString, MultiPoint, MultiPolygon, Point, Polygon};
    use std::{f64::consts::TAU, str::FromStr};

    /// The reference point every Bremen fixture is placed around.
    const BREMEN: (f64, f64) = (8.8072, 53.0758);

    fn metres(value: &str) -> PositiveDistance {
        PositiveDistance::from_str(value).expect("positive distance")
    }

    fn point(longitude: f64, latitude: f64) -> Geometry<f64> {
        Geometry::Point(Point::new(longitude, latitude))
    }

    fn bremen() -> Geometry<f64> {
        point(BREMEN.0, BREMEN.1)
    }

    fn box_polygon(west: f64, south: f64, east: f64, north: f64) -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![(west, south), (east, south), (east, north), (west, north), (west, south)]),
            Vec::new(),
        )
    }

    /// A square east of Bremen whose west wall is 100.06 m from the reference
    /// point on the spheroid and whose centroid is 170.10 m from it.
    fn east_square() -> Polygon<f64> {
        box_polygon(8.808_693, 53.075_171, 8.810_783, 53.076_429)
    }

    /// Whether `subject` comes within `distance` of `reference`, threshold
    /// included, read both ways round.
    fn within_inclusive(subject: &Geometry<f64>, reference: &Geometry<f64>, distance: &str) -> bool {
        let forward = Proximity::between(subject, reference)
            .expect("non-empty geometries")
            .comes_within(metres(distance), Boundary::Inclusive);
        let backward = Proximity::between(reference, subject)
            .expect("non-empty geometries")
            .comes_within(metres(distance), Boundary::Inclusive);
        pretty_assertions::assert_eq!(forward, backward, "the separation is symmetric");
        forward
    }

    /// Assert that the separation of the two geometries lies strictly between
    /// `below` and `above` metres.
    fn assert_separation_between(subject: &Geometry<f64>, reference: &Geometry<f64>, below: &str, above: &str) {
        assert!(!within_inclusive(subject, reference, below), "the geometries come within {below} m");
        assert!(within_inclusive(subject, reference, above), "the geometries do not come within {above} m");
    }

    // The separations below were checked against `PostGIS` `ST_Distance`
    // over `geography`, which gives the same value to the millimetre.

    #[test]
    fn a_polygon_is_as_close_as_its_nearest_wall_not_its_centroid() {
        // 100.06 m to the west wall; 170.10 m to the centroid.
        assert_separation_between(&Geometry::Polygon(east_square()), &bremen(), "100.05", "100.07");
    }

    #[test]
    fn a_line_is_as_close_as_the_foot_of_the_perpendicular_on_its_nearest_edge() {
        // The nearest vertex is 316.70 m away, the foot on the edge 100.06 m.
        let line = Geometry::LineString(LineString::from(vec![(8.808_693, 53.0731), (8.808_693, 53.0785), (8.817_693, 53.0785)]));
        assert_separation_between(&line, &bremen(), "100.05", "100.07");
    }

    #[test]
    fn a_multi_polygon_is_as_close_as_its_nearest_member() {
        // The other member's nearest wall is 2 003.89 m away.
        let pair = Geometry::MultiPolygon(MultiPolygon::new(vec![east_square(), box_polygon(8.7753, 53.0748, 8.7773, 53.0768)]));
        assert_separation_between(&pair, &bremen(), "100.05", "100.07");
    }

    #[test]
    fn a_point_inside_a_polygon_touches_it() {
        // The polygon's centroid is 995.24 m away.
        let area = Geometry::Polygon(box_polygon(8.8069, 53.0753, 8.8372, 53.0763));
        assert!(within_inclusive(&area, &bremen(), "0.001"));
    }

    #[test]
    fn a_point_in_a_polygon_hole_is_as_close_as_the_hole_boundary() {
        // The hole's east wall is the closest on the sphere (99.73 m) and
        // measures 100.06 m on the spheroid, as in `PostGIS`; its north wall
        // would measure 99.94 m on the spheroid but is farther on the sphere.
        let (x, y) = BREMEN;
        let outer = LineString::from(vec![
            (x - 0.015, y - 0.009),
            (x + 0.015, y - 0.009),
            (x + 0.015, y + 0.009),
            (x - 0.015, y + 0.009),
            (x - 0.015, y - 0.009),
        ]);
        let (half_width, half_height) = (0.001_493, 0.000_898);
        let hole = LineString::from(vec![
            (x - half_width, y - half_height),
            (x + half_width, y - half_height),
            (x + half_width, y + half_height),
            (x - half_width, y + half_height),
            (x - half_width, y - half_height),
        ]);
        let holed = Geometry::Polygon(Polygon::new(outer, vec![hole]));
        assert_separation_between(&holed, &bremen(), "100.05", "100.07");
    }

    #[test]
    fn a_polygon_reference_is_measured_from_its_nearest_edge() {
        // 90.07 m from the square's east edge; 140.07 m from its centre.
        let square = Geometry::Polygon(box_polygon(8.806_454, 53.075_351, 8.807_946, 53.076_249));
        assert_separation_between(&point(8.809_290, 53.0758), &square, "90.06", "90.08");
    }

    #[test]
    fn overlapping_polygons_touch() {
        let shifted = box_polygon(8.808_193, 53.075_171, 8.810_283, 53.076_429);
        assert!(within_inclusive(&Geometry::Polygon(east_square()), &Geometry::Polygon(shifted), "0.001"));
    }

    #[test]
    fn a_polygon_cut_at_the_antimeridian_is_measured_across_it() {
        // 212.97 m across the antimeridian; its planar centroid is 692.16 m away.
        let cut = Geometry::MultiPolygon(MultiPolygon::new(vec![box_polygon(-179.999, -17.0005, -179.990, -16.9995)]));
        assert_separation_between(&cut, &point(179.999, -17.0), "212.96", "212.98");
    }

    #[test]
    fn points_on_either_side_of_the_antimeridian_are_close() {
        assert_separation_between(&point(-179.9995, -17.0), &point(179.9995, -17.0), "106.48", "106.49");
    }

    #[test]
    fn points_either_side_of_a_pole_are_measured_over_it() {
        assert_separation_between(&point(180.0, 89.9), &point(0.0, 89.9), "22338.7", "22338.9");
    }

    #[test]
    fn nearly_antipodal_points_are_measured_to_the_metre() {
        assert_separation_between(&point(179.5, 0.5), &point(0.0, 0.0), "19936288", "19936289");
    }

    #[test]
    fn the_separation_is_measured_on_the_spheroid_not_the_sphere() {
        // 5 000.17 m on the spheroid; the sphere says 5 028.24 m.
        assert_separation_between(&point(0.0, 0.045_22), &point(0.0, 0.0), "5000.1", "5000.2");
    }

    #[test]
    fn a_long_edge_is_measured_along_its_great_circle() {
        // The edge bows north of the 60th parallel to 63.435 degrees, which
        // `PostGIS` measures the point as 551.64 m from.
        let edge = Geometry::LineString(LineString::from(vec![(-30.0, 60.0), (30.0, 60.0)]));
        assert_separation_between(&edge, &point(0.0, 63.43), "551.6", "551.7");
    }

    #[test]
    fn an_empty_geometry_has_no_proximity() {
        let empty_polygon = Geometry::Polygon(Polygon::new(LineString::new(Vec::new()), Vec::new()));
        let empty_points = Geometry::MultiPoint(MultiPoint::new(Vec::new()));
        for empty in [&empty_polygon, &empty_points] {
            assert!(Proximity::between(empty, &bremen()).is_none(), "{empty:?}");
            assert!(Proximity::between(&bremen(), empty).is_none(), "{empty:?}");
        }
    }

    #[test]
    fn an_empty_member_of_a_multi_geometry_is_skipped() {
        // `geo`'s own closest point of a multi-polygon is indeterminate as soon
        // as one member is empty.
        let with_empty = Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new(LineString::new(Vec::new()), Vec::new()), east_square()]));
        assert_separation_between(&with_empty, &bremen(), "100.05", "100.07");
    }

    #[test]
    fn a_separation_equal_to_the_distance_is_within_it_only_inclusively() {
        assert!(within(100.0, || 100.0, 100.0, Boundary::Inclusive));
        assert!(!within(100.0, || 100.0, 100.0, Boundary::Exclusive));
        assert!(within(100.0, || 99.999, 100.0, Boundary::Exclusive));
    }

    #[test]
    fn the_geodesic_is_computed_only_inside_the_band_the_sphere_cannot_decide() {
        let unreachable = || -> f64 { panic!("the spherical distance decides this pair alone") };
        assert!(within(90.0, unreachable, 100.0, Boundary::Inclusive));
        assert!(!within(110.0, unreachable, 100.0, Boundary::Inclusive));
        assert!(within(90.0, unreachable, 100.0, Boundary::Exclusive));
        assert!(!within(110.0, unreachable, 100.0, Boundary::Exclusive));
    }

    #[test]
    fn the_spheroid_to_sphere_ratio_stays_inside_its_bounds_across_the_globe() {
        let mut latitude = -88.0_f64;
        while latitude <= 88.0 {
            let mut longitude = -180.0_f64;
            while longitude <= 180.0 {
                for (other_longitude, other_latitude) in [(longitude + 3.3, latitude + 1.7), (longitude + 97.0, -latitude), (longitude + 0.01, latitude)] {
                    let other_longitude = if other_longitude > 180.0 { other_longitude - 360.0 } else { other_longitude };
                    let (a, b) = (Point::new(longitude, latitude), Point::new(other_longitude, other_latitude.clamp(-90.0, 90.0)));
                    let spherical = Haversine.distance(a, b);
                    let geodesic = Geodesic.distance(a, b);
                    assert!(
                        spherical * SPHEROID_OVER_SPHERE_MIN <= geodesic && geodesic <= spherical * SPHEROID_OVER_SPHERE_MAX,
                        "{a:?} to {b:?}: geodesic {geodesic} against spherical {spherical}"
                    );
                }
                longitude += 20.0;
            }
            latitude += 8.0;
        }
    }

    /// A closed ring of `vertices` positions approximating a circle.
    fn ring(centre_longitude: f64, centre_latitude: f64, radius: f64, vertices: u32) -> Polygon<f64> {
        let mut positions: Vec<(f64, f64)> = (0..vertices)
            .map(|vertex| {
                let angle = f64::from(vertex) / f64::from(vertices) * TAU;
                (centre_longitude + radius * angle.cos(), centre_latitude + radius * angle.sin())
            })
            .collect();
        positions.extend(positions.first().copied());
        Polygon::new(LineString::from(positions), Vec::new())
    }

    #[test]
    fn two_large_polygons_side_by_side_are_decided_without_an_all_pairs_scan() {
        // Two 5 000-vertex polygons whose nearest points are 50.35 m apart.
        // An all-pairs scan measures 25 million vertex-arc pairs per
        // direction; the pruned search measures a handful per vertex.
        let left = Geometry::Polygon(ring(8.0, 53.0, 0.1, 5_000));
        let right = Geometry::Polygon(ring(8.200_75, 53.0, 0.1, 5_000));
        assert_separation_between(&left, &right, "40", "150");
    }

    #[test]
    fn two_large_polygons_far_apart_are_decided_without_an_all_pairs_scan() {
        // 20.14 km apart, each about 13 km across: a 25 km threshold reaches
        // most of both, and the first pair clearly within it settles it.
        let left = Geometry::Polygon(ring(8.0, 53.0, 0.1, 5_000));
        let right = Geometry::Polygon(ring(8.5, 53.0, 0.1, 5_000));
        assert_separation_between(&left, &right, "20000", "25000");
    }
}
