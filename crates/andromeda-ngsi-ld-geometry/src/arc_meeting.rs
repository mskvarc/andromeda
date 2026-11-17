//! Whether two great-circle arcs share a point.
//!
//! `near` reads every edge as the minor great-circle arc between its ends (see
//! [`crate::great_circle`]), so two geometries touch where two of their arcs
//! meet. An arc lies in the plane through the centre of the earth normal to the
//! cross product of its ends. Two arcs in different planes can only meet where
//! the planes cross, at one of the two antipodal points the cross product of
//! their normals names, and they meet when one of those points lies on both
//! arcs. That is the whole test: an end of one arc lying on the other's great
//! circle outside the other arc is no meeting, whichever side of the first arc's
//! plane the other arc's ends lie on. Two arcs in one plane meet when an end of
//! one lies on the other.
//!
//! An arc whose ends coincide is that one point. An arc whose ends are
//! antipodal has no single shortest path between them, and is read as its two
//! ends.

use crate::sphere_vector::SphereVector;
use geo::Line;

/// How far a point may lie from an arc and still be on it, as the sine of the
/// angle at the centre of the earth: about 6 µm on the ground, the tolerance
/// `PostGIS` gives the same test.
const ON_ARC_TOLERANCE: f64 = 1e-12;

/// An arc as its ends determine it.
#[derive(Debug, Clone, Copy)]
enum ArcShape {
    /// Ends that coincide.
    Point(SphereVector),
    /// Antipodal ends, which no single shortest path joins.
    Ends(SphereVector, SphereVector),
    /// The minor arc from `start` to `end`, in the plane normal to the unit
    /// vector `normal`.
    Minor {
        /// The first end.
        start: SphereVector,
        /// The second end.
        end: SphereVector,
        /// The unit normal of the arc's plane, `start x end` normalised.
        normal: SphereVector,
    },
}

impl ArcShape {
    /// The shape of the arc from `arc.start` to `arc.end`, positions in
    /// degrees of longitude and latitude.
    fn of(arc: Line<f64>) -> ArcShape {
        let (start, end) = (SphereVector::of(arc.start), SphereVector::of(arc.end));
        match start.cross(end).normalized() {
            Some(normal) => ArcShape::Minor { start, end, normal },
            None if start.dot(end) > 0.0 => ArcShape::Point(start),
            None => ArcShape::Ends(start, end),
        }
    }

    /// Whether `point` lies on the arc.
    fn holds(self, point: SphereVector) -> bool {
        match self {
            ArcShape::Point(only) => coincide(only, point),
            ArcShape::Ends(first, second) => coincide(first, point) || coincide(second, point),
            ArcShape::Minor { start, end, normal } => normal.dot(point).abs() <= ON_ARC_TOLERANCE && between(start, end, normal, point),
        }
    }
}

/// Whether the arcs `first` and `second` share a point.
pub(crate) fn arcs_meet(first: Line<f64>, second: Line<f64>) -> bool {
    match (ArcShape::of(first), ArcShape::of(second)) {
        (ArcShape::Point(point), other) | (other, ArcShape::Point(point)) => other.holds(point),
        (ArcShape::Ends(one, other_end), other) | (other, ArcShape::Ends(one, other_end)) => other.holds(one) || other.holds(other_end),
        (
            first @ ArcShape::Minor {
                start: first_start,
                end: first_end,
                normal: first_normal,
            },
            second @ ArcShape::Minor {
                start: second_start,
                end: second_end,
                normal: second_normal,
            },
        ) => match plane_crossing(first_normal, second_normal) {
            Some(crossing) => [crossing, crossing.antipode()]
                .into_iter()
                .any(|candidate| between(first_start, first_end, first_normal, candidate) && between(second_start, second_end, second_normal, candidate)),
            None => first.holds(second_start) || first.holds(second_end) || second.holds(first_start) || second.holds(first_end),
        },
    }
}

/// One of the two antipodal points where the great circles normal to
/// `first_normal` and `second_normal` cross, or `None` when one great circle
/// carries both.
fn plane_crossing(first_normal: SphereVector, second_normal: SphereVector) -> Option<SphereVector> {
    let crossing = first_normal.cross(second_normal);
    (crossing.dot(crossing) > ON_ARC_TOLERANCE * ON_ARC_TOLERANCE)
        .then(|| crossing.normalized())
        .flatten()
}

/// Whether `point`, a point of the great circle normal to `normal`, lies
/// between `start` and `end` on the minor arc from one to the other.
///
/// Turning from `start` towards `end` about `normal`, the point is reached
/// within half a turn and `end` within half a turn after it.
fn between(start: SphereVector, end: SphereVector, normal: SphereVector, point: SphereVector) -> bool {
    start.cross(point).dot(normal) >= -ON_ARC_TOLERANCE && point.cross(end).dot(normal) >= -ON_ARC_TOLERANCE
}

/// Whether two unit vectors name the same point.
fn coincide(first: SphereVector, second: SphereVector) -> bool {
    let gap = first.minus(second);
    gap.dot(gap) <= ON_ARC_TOLERANCE * ON_ARC_TOLERANCE
}

#[cfg(test)]
mod tests {
    use crate::arc_meeting::arcs_meet;
    use geo::Line;

    fn arc(start: (f64, f64), end: (f64, f64)) -> Line<f64> {
        Line::new(start, end)
    }

    /// Whether the arcs meet, asserted to be the same whichever is named first.
    fn meet(first: Line<f64>, second: Line<f64>) -> bool {
        let forward = arcs_meet(first, second);
        pretty_assertions::assert_eq!(forward, arcs_meet(second, first), "meeting is symmetric");
        forward
    }

    #[test]
    fn two_arcs_crossing_in_their_middles_meet() {
        assert!(meet(arc((-10.0, 0.0), (10.0, 0.0)), arc((0.0, -10.0), (0.0, 10.0))));
    }

    #[test]
    fn two_arcs_sharing_an_end_meet() {
        assert!(meet(arc((0.0, 0.0), (10.0, 10.0)), arc((10.0, 10.0), (20.0, 0.0))));
    }

    #[test]
    fn an_arc_ending_on_another_meets_it() {
        assert!(meet(arc((0.0, 0.0), (0.0, 10.0)), arc((-10.0, 0.0), (10.0, 0.0))));
    }

    #[test]
    fn arcs_whose_ends_are_antipodal_do_not_meet() {
        // (-170, -10) is the antipode of (10, 10).
        assert!(!meet(arc((170.0, -10.0), (-170.0, -10.0)), arc((10.0, 10.0), (0.0, 10.0))));
        assert!(!meet(arc((-10.0, -10.0), (10.0, -10.0)), arc((170.0, 10.0), (180.0, 10.0))));
    }

    #[test]
    fn an_end_on_the_other_great_circle_beyond_the_other_arc_is_no_meeting() {
        // (0, 0) lies on the equator, which the second arc follows across the
        // antimeridian, through (180, 0), the antipode of (0, 0).
        assert!(!meet(arc((0.0, 0.0), (0.0, 10.0)), arc((170.0, 0.0), (-170.0, 0.0))));
    }

    #[test]
    fn an_arc_across_the_antimeridian_goes_the_short_way() {
        assert!(meet(arc((170.0, -10.0), (-170.0, -10.0)), arc((180.0, -20.0), (180.0, 0.0))));
        assert!(!meet(arc((170.0, -10.0), (-170.0, -10.0)), arc((0.0, -20.0), (0.0, 0.0))));
    }

    #[test]
    fn a_long_arc_meets_where_it_bows() {
        // The arc along the 60th parallel bows north to 63.43 degrees.
        assert!(meet(arc((-30.0, 60.0), (30.0, 60.0)), arc((0.0, 62.0), (0.0, 64.0))));
        assert!(!meet(arc((-30.0, 60.0), (30.0, 60.0)), arc((0.0, 63.5), (0.0, 64.0))));
    }

    #[test]
    fn arcs_of_one_great_circle_meet_only_where_they_overlap() {
        assert!(meet(arc((0.0, 0.0), (20.0, 0.0)), arc((10.0, 0.0), (30.0, 0.0))));
        assert!(meet(arc((0.0, 0.0), (20.0, 0.0)), arc((20.0, 0.0), (30.0, 0.0))));
        assert!(!meet(arc((0.0, 0.0), (20.0, 0.0)), arc((21.0, 0.0), (30.0, 0.0))));
        assert!(!meet(arc((0.0, 0.0), (20.0, 0.0)), arc((180.0, 0.0), (190.0, 0.0))));
    }

    #[test]
    fn a_point_meets_an_arc_through_it_only() {
        assert!(meet(arc((5.0, 0.0), (5.0, 0.0)), arc((0.0, 0.0), (10.0, 0.0))));
        assert!(!meet(arc((5.0, 0.1), (5.0, 0.1)), arc((0.0, 0.0), (10.0, 0.0))));
        assert!(!meet(arc((-175.0, 0.0), (-175.0, 0.0)), arc((0.0, 0.0), (10.0, 0.0))));
    }

    #[test]
    fn two_points_meet_when_they_coincide() {
        assert!(meet(arc((180.0, 10.0), (180.0, 10.0)), arc((-180.0, 10.0), (-180.0, 10.0))));
        assert!(!meet(arc((180.0, 10.0), (180.0, 10.0)), arc((179.0, 10.0), (179.0, 10.0))));
    }

    #[test]
    fn an_arc_between_antipodal_positions_is_its_two_ends() {
        let antipodal = arc((0.0, 0.0), (180.0, 0.0));
        assert!(meet(antipodal, arc((180.0, -5.0), (180.0, 5.0))));
        assert!(!meet(antipodal, arc((90.0, -5.0), (90.0, 5.0))));
    }

    #[test]
    fn an_arc_spanning_exactly_180_degrees_of_longitude_runs_over_the_nearer_pole() {
        let over_the_north_pole = arc((0.0, 10.0), (180.0, 20.0));
        assert!(meet(over_the_north_pole, arc((-90.0, 89.0), (90.0, 89.0))));
        assert!(!meet(over_the_north_pole, arc((-90.0, -89.0), (90.0, -89.0))));
    }
}
