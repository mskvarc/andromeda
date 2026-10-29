//! The box a store indexes a geometry by.
//!
//! A geometry index keys a stored geometry by its planar bounding rectangle,
//! which bounds its vertices alone. `near` measures along great-circle edges,
//! and a long edge bows past its vertices (see [`crate::great_circle`]), so a
//! box test against the planar rectangle can discard a geometry the distance
//! keeps. The reach is the rectangle the great-circle edges cover, kept beside
//! the geometry only where it extends past the planar rectangle by more than
//! [`REACH_TOLERANCE_DEGREES`]: never for a point or a meridian edge, and never
//! for an edge shorter than about 2 km. [`crate::near_envelope::NearEnvelope`]
//! is widened by the same tolerance, so a bulge too small to keep is still
//! covered.
//!
//! The reach contains the planar rectangle, so a planar relation tested
//! against it discards nothing it would match either.

use crate::{
    great_circle::{arcs, great_circle_bounds},
    planar::PlanarGeometry,
};
use geo::{Coord, Rect};

/// How far, in degrees, a great-circle reach may extend past the planar
/// bounding rectangle on any side and still be left out: about 11 cm on the
/// ground.
pub const REACH_TOLERANCE_DEGREES: f64 = 1e-6;

/// The longitude/latitude rectangle covering a geometry read with
/// great-circle edges, where it extends past the planar bounding rectangle by
/// more than [`REACH_TOLERANCE_DEGREES`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GreatCircleReach(Rect<f64>);

impl GreatCircleReach {
    /// The reach of `geometry`, or `None` when its great-circle bounds stay
    /// within [`REACH_TOLERANCE_DEGREES`] of its planar bounding rectangle on
    /// every side, or when it has no coordinates.
    #[must_use]
    pub fn of(geometry: &PlanarGeometry) -> Option<GreatCircleReach> {
        let planar = geometry.bounding_rect()?;
        let reach = repair_reach(geometry, great_circle_bounds(geometry)?);
        let overhang = [
            planar.min().x - reach.min().x,
            planar.min().y - reach.min().y,
            reach.max().x - planar.max().x,
            reach.max().y - planar.max().y,
        ];
        overhang
            .into_iter()
            .any(|degrees| degrees > REACH_TOLERANCE_DEGREES)
            .then_some(GreatCircleReach(reach))
    }
}

/// `bounds` grown to cover every form `ST_MakeValid` can repair the geometry
/// into.
///
/// The repair nodes edges on the plane, adding vertices on them. A sub-arc
/// between two points of an edge that keeps to less than half the globe in
/// longitude stays inside that edge's arc bounds. An edge whose ends lie more
/// than 180 degrees of longitude apart is read as an arc across the
/// antimeridian, but a vertex the repair adds lies on the planar segment the
/// long way round, and the sub-arcs to it can bow to a pole: from (-170, -50)
/// to (0, -60) the arc reaches -87.8. Such an edge therefore extends the
/// bounds to the pole of each hemisphere its ends lie in.
fn repair_reach(geometry: &PlanarGeometry, bounds: Rect<f64>) -> Rect<f64> {
    arcs(geometry.as_geo())
        .into_iter()
        .filter(|arc| (arc.end.x - arc.start.x).abs() > 180.0)
        .fold(bounds, |bounds, arc| {
            let south = if arc.start.y.min(arc.end.y) < 0.0 { -90.0 } else { bounds.min().y };
            let north = if arc.start.y.max(arc.end.y) > 0.0 { 90.0 } else { bounds.max().y };
            Rect::new(Coord { x: bounds.min().x, y: south }, Coord { x: bounds.max().x, y: north })
        })
}

impl GreatCircleReach {
    /// The western longitude.
    #[must_use]
    pub fn west(&self) -> f64 {
        self.0.min().x
    }

    /// The southern latitude.
    #[must_use]
    pub fn south(&self) -> f64 {
        self.0.min().y
    }

    /// The eastern longitude.
    #[must_use]
    pub fn east(&self) -> f64 {
        self.0.max().x
    }

    /// The northern latitude.
    #[must_use]
    pub fn north(&self) -> f64 {
        self.0.max().y
    }
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
        great_circle_reach::{GreatCircleReach, REACH_TOLERANCE_DEGREES},
        line_string_coordinates::LineStringCoordinates,
        linear_ring::LinearRing,
        position::Position,
    };

    fn position(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn line(positions: &[(f64, f64)]) -> Geometry {
        Geometry::new(GeometryForm::LineString(
            LineStringCoordinates::try_new(positions.iter().map(|&(longitude, latitude)| position(longitude, latitude)).collect())
                .expect("a test line has two positions or more"),
        ))
    }

    fn polygon(ring: &[(f64, f64)]) -> Geometry {
        let ring = LinearRing::try_new(ring.iter().map(|&(longitude, latitude)| position(longitude, latitude)).collect()).expect("a closed test ring");
        Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![ring])))
    }

    fn reach(geometry: &Geometry) -> Option<GreatCircleReach> {
        GreatCircleReach::of(&geometry.planar())
    }

    #[test]
    fn a_point_has_no_reach() {
        assert!(reach(&Geometry::new(GeometryForm::Point(position(8.8072, 53.0758)))).is_none());
    }

    #[test]
    fn a_one_kilometre_edge_at_45_north_has_no_reach() {
        // 1 km of longitude at 45 N is 0.0127 degrees; the arc bows by about
        // 3e-7 degrees, under the tolerance.
        assert!(reach(&line(&[(10.0, 45.0), (10.0127, 45.0)])).is_none());
    }

    #[test]
    fn a_meridian_edge_has_no_reach() {
        assert!(reach(&line(&[(10.0, -60.0), (10.0, 70.0)])).is_none());
    }

    #[test]
    fn an_edge_along_the_sixtieth_parallel_reaches_63_43_north() {
        let reach = reach(&line(&[(0.0, 60.0), (60.0, 60.0)])).expect("a long edge bows past its ends");
        assert!((reach.north() - 63.434_948_8).abs() < 1e-6, "north {}", reach.north());
        pretty_assertions::assert_eq!((reach.west(), reach.south(), reach.east()), (0.0, 60.0, 60.0));
    }

    #[test]
    fn an_edge_across_the_antimeridian_covers_both_sides() {
        let reach = reach(&line(&[(179.9, 10.0), (-179.9, 10.0)])).expect("the planar box misses the crossing");
        pretty_assertions::assert_eq!((reach.west(), reach.east()), (-180.0, 180.0));
    }

    /// `ST_MakeValid` repairs a polygon by noding its edges: it adds vertices
    /// on them, never off them. A sub-arc between two points of an edge
    /// stays inside the edge's reach, so the reach of the written polygon
    /// bounds every repaired form a relation judges.
    #[test]
    fn a_polygon_with_vertices_added_on_its_edges_stays_inside_its_reach() {
        let rings: [&[(f64, f64)]; 3] = [
            // A bowtie crossing itself at (0, 62).
            &[(-40.0, 55.0), (40.0, 69.0), (40.0, 55.0), (-40.0, 69.0), (-40.0, 55.0)],
            &[(-170.0, -50.0), (170.0, -70.0), (170.0, -50.0), (-170.0, -70.0), (-170.0, -50.0)],
            &[(0.0, 10.0), (90.0, 50.0), (0.0, 50.0), (90.0, 10.0), (0.0, 10.0)],
        ];
        for ring in rings {
            let written = polygon(ring);
            let bounds = reach(&written).expect("long edges bow past their ends");
            for parts in [2_u32, 3, 7, 50] {
                let noded: Vec<(f64, f64)> = ring
                    .iter()
                    .zip(ring.iter().skip(1))
                    .flat_map(|(start, end)| {
                        (0..parts).map(move |step| {
                            let ratio = f64::from(step) / f64::from(parts);
                            (start.0 + (end.0 - start.0) * ratio, start.1 + (end.1 - start.1) * ratio)
                        })
                    })
                    .chain(ring.first().copied())
                    .collect();
                let repaired = great_circle_bounds(&polygon(&noded).planar()).expect("a noded ring has bounds");
                assert!(
                    repaired.min().x >= bounds.west() - REACH_TOLERANCE_DEGREES
                        && repaired.min().y >= bounds.south() - REACH_TOLERANCE_DEGREES
                        && repaired.max().x <= bounds.east() + REACH_TOLERANCE_DEGREES
                        && repaired.max().y <= bounds.north() + REACH_TOLERANCE_DEGREES,
                    "{ring:?} noded in {parts}: {repaired:?} escapes {bounds:?}"
                );
            }
        }
    }
}
