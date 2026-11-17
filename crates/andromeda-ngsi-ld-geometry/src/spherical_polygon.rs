//! A polygon read the way `near` reads it: every edge a great-circle arc, and
//! the inside the region its rings enclose within one hemisphere.
//!
//! A closed ring of great-circle arcs divides the sphere into two regions, and a
//! `GeoJSON` ring does not say which one it bounds: RFC 7946 section 3.1.6 asks
//! for counterclockwise exterior rings but tells parsers not to reject the other
//! winding. The polygon is taken to be the region inside the hemisphere centred
//! on the middle of the box its positions span in three-dimensional space, which
//! is the reading `PostGIS` gives `geography`. So the ring
//! `[[170,-10],[-170,-10],[-170,10],[170,10],[170,-10]]` is the polygon 20
//! degrees wide around the antimeridian, a polygon whose north edge runs along a
//! parallel holds the band its great-circle edge bows over, and a ring along the
//! 80th parallel holds the north pole.
//!
//! The gnomonic projection centred on that hemisphere maps every great circle
//! to a straight line, so the polygon projects to a planar polygon with the same
//! edges and the same inside, and a position lies in the polygon exactly when
//! its projection lies in the projected one. A polygon whose positions do not
//! all lie well inside that hemisphere has no such projection and is read with
//! straight edges in longitude and latitude, as written.

use crate::sphere_vector::SphereVector;
use geo::{Coord, Intersects, LineString, Point, Polygon};
use std::{borrow::Cow, iter::once};

/// How far inside the hemisphere every position of a projected polygon must
/// lie, as the cosine of its angle from the centre: about 0.000 06 degrees
/// short of the horizon, where projected coordinates reach a million.
const HORIZON_MARGIN: f64 = 1e-6;

/// One polygon of a geometry, ready to answer whether it holds a position.
#[derive(Debug)]
pub(crate) struct SphericalPolygon<'a>(Reading<'a>);

/// How a polygon is read.
#[derive(Debug)]
enum Reading<'a> {
    /// The polygon has no rings and holds nothing.
    Empty,
    /// The polygon fits within a hemisphere and is read through the gnomonic
    /// projection centred on it.
    Gnomonic {
        /// The projection.
        projection: Gnomonic,
        /// The polygon's projection.
        projected: Polygon<f64>,
    },
    /// The polygon is too large for a hemisphere and is read as written.
    Planar(Cow<'a, Polygon<f64>>),
}

impl<'a> SphericalPolygon<'a> {
    /// The reading of `polygon`, whose positions are in degrees of longitude
    /// and latitude.
    pub(crate) fn of(polygon: Cow<'a, Polygon<f64>>) -> SphericalPolygon<'a> {
        if polygon.exterior().0.is_empty() {
            return SphericalPolygon(Reading::Empty);
        }
        let rings: Vec<Vec<SphereVector>> = once(polygon.exterior())
            .chain(polygon.interiors())
            .map(|ring| ring.coords().copied().map(SphereVector::of).collect())
            .collect();
        let projected = Gnomonic::centred_on(rings.iter().flatten().copied()).and_then(|projection| {
            let mut projected_rings = rings.iter().map(|ring| {
                ring.iter()
                    .map(|position| projection.project(*position))
                    .collect::<Option<Vec<Coord<f64>>>>()
                    .map(LineString)
            });
            let exterior = projected_rings.next()??;
            let interiors = projected_rings.collect::<Option<Vec<LineString<f64>>>>()?;
            Some(Reading::Gnomonic {
                projection,
                projected: Polygon::new(exterior, interiors),
            })
        });
        SphericalPolygon(projected.unwrap_or(Reading::Planar(polygon)))
    }

    /// Whether the polygon holds `position`, on its boundary included.
    pub(crate) fn covers(&self, position: Coord<f64>) -> bool {
        match &self.0 {
            Reading::Empty => false,
            Reading::Gnomonic { projection, projected } => projection
                .project(SphereVector::of(position))
                .is_some_and(|projected_position| projected.intersects(&Point::from(projected_position))),
            Reading::Planar(polygon) => polygon.intersects(&Point::from(position)),
        }
    }
}

/// The polygons of a geometry, a rectangle or a triangle read as the polygon
/// it is.
pub(crate) fn polygons_of(geometry: &geo::Geometry<f64>) -> Vec<Cow<'_, Polygon<f64>>> {
    let mut collected = Vec::new();
    collect_polygons(geometry, &mut collected);
    collected
}

/// Append the polygons of `geometry` to `collected`.
fn collect_polygons<'a>(geometry: &'a geo::Geometry<f64>, collected: &mut Vec<Cow<'a, Polygon<f64>>>) {
    match geometry {
        geo::Geometry::Point(_) | geo::Geometry::MultiPoint(_) | geo::Geometry::Line(_) | geo::Geometry::LineString(_) | geo::Geometry::MultiLineString(_) => {}
        geo::Geometry::Polygon(polygon) => collected.push(Cow::Borrowed(polygon)),
        geo::Geometry::MultiPolygon(polygons) => collected.extend(polygons.iter().map(Cow::Borrowed)),
        geo::Geometry::Rect(rect) => collected.push(Cow::Owned(rect.to_polygon())),
        geo::Geometry::Triangle(triangle) => collected.push(Cow::Owned(triangle.to_polygon())),
        geo::Geometry::GeometryCollection(members) => members.iter().for_each(|member| collect_polygons(member, collected)),
    }
}

/// The gnomonic projection centred on one point of the sphere, onto the plane
/// tangent to it there.
#[derive(Debug, Clone, Copy)]
struct Gnomonic {
    /// The point of tangency.
    centre: SphereVector,
    /// The unit vector of the plane's first axis.
    east: SphereVector,
    /// The unit vector of the plane's second axis.
    north: SphereVector,
}

impl Gnomonic {
    /// The projection centred on the middle of the box `positions` span, when
    /// every position lies well inside the hemisphere around it.
    fn centred_on(mut positions: impl Iterator<Item = SphereVector> + Clone) -> Option<Gnomonic> {
        let (least, greatest) = positions
            .clone()
            .map(|position| (position, position))
            .reduce(|(least, greatest), (position, _)| {
                let (lower, _) = least.extremes(position);
                let (_, upper) = greatest.extremes(position);
                (lower, upper)
            })?;
        let centre = least.midpoint(greatest).normalized()?;
        // At a pole every direction is south; any axis perpendicular to the
        // centre serves.
        let east = SphereVector::NORTH_POLE.cross(centre).normalized().unwrap_or(SphereVector::new(0.0, 1.0, 0.0));
        let projection = Gnomonic {
            centre,
            east,
            north: centre.cross(east),
        };
        positions.all(|position| centre.dot(position) > HORIZON_MARGIN).then_some(projection)
    }

    /// The projection of `position`, or `None` for a position on or beyond
    /// the horizon, which the projection does not reach.
    fn project(&self, position: SphereVector) -> Option<Coord<f64>> {
        let height = self.centre.dot(position);
        (height > HORIZON_MARGIN).then(|| Coord {
            x: self.east.dot(position) / height,
            y: self.north.dot(position) / height,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::spherical_polygon::SphericalPolygon;
    use geo::{Coord, LineString, Polygon};
    use std::borrow::Cow;

    fn polygon(exterior: &[(f64, f64)], holes: &[&[(f64, f64)]]) -> Polygon<f64> {
        Polygon::new(
            LineString::from(exterior.to_vec()),
            holes.iter().map(|hole| LineString::from(hole.to_vec())).collect(),
        )
    }

    fn covers(polygon: &Polygon<f64>, longitude: f64, latitude: f64) -> bool {
        SphericalPolygon::of(Cow::Borrowed(polygon)).covers(Coord { x: longitude, y: latitude })
    }

    #[test]
    fn a_ring_with_edges_across_the_antimeridian_is_the_polygon_20_degrees_wide() {
        let ring = polygon(&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)], &[]);
        for (longitude, latitude) in [(180.0, 0.0), (-180.0, 0.0), (-175.0, 5.0), (175.0, -5.0), (170.0, 0.0)] {
            assert!(covers(&ring, longitude, latitude), "({longitude}, {latitude}) lies inside");
        }
        for (longitude, latitude) in [(0.0, 0.0), (90.0, 0.0), (169.0, 0.0), (5.0, 5.0), (10.0, 10.0)] {
            assert!(!covers(&ring, longitude, latitude), "({longitude}, {latitude}) lies outside");
        }
    }

    #[test]
    fn a_polygon_holds_the_band_its_long_edge_bows_over() {
        // The north edge along the 60th parallel bows north to 63.43 degrees.
        let area = polygon(&[(-30.0, 50.0), (30.0, 50.0), (30.0, 60.0), (-30.0, 60.0), (-30.0, 50.0)], &[]);
        assert!(covers(&area, 0.0, 61.0));
        assert!(covers(&area, 0.0, 63.0));
        assert!(!covers(&area, 0.0, 64.0));
        assert!(!covers(&area, 0.0, 49.0));
    }

    #[test]
    fn a_ring_along_a_parallel_holds_its_pole() {
        let cap = polygon(&[(0.0, 80.0), (90.0, 80.0), (180.0, 80.0), (-90.0, 80.0), (0.0, 80.0)], &[]);
        assert!(covers(&cap, 0.0, 90.0));
        assert!(covers(&cap, 45.0, 85.0));
        assert!(!covers(&cap, 0.0, 70.0));
        assert!(!covers(&cap, 0.0, -90.0));
    }

    #[test]
    fn either_winding_of_a_ring_is_the_same_polygon() {
        let counterclockwise = polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)], &[]);
        let clockwise = polygon(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0), (0.0, 0.0)], &[]);
        for polygon in [&counterclockwise, &clockwise] {
            assert!(covers(polygon, 5.0, 5.0));
            assert!(!covers(polygon, -175.0, -5.0));
        }
    }

    #[test]
    fn a_hole_holds_nothing_of_the_polygon() {
        let holed = polygon(
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)],
            &[&[(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0), (4.0, 4.0)]],
        );
        assert!(!covers(&holed, 5.0, 5.0));
        assert!(covers(&holed, 2.0, 2.0));
        assert!(covers(&holed, 4.0, 5.0), "the hole's boundary is the polygon's");
    }

    #[test]
    fn a_polygon_too_large_for_a_hemisphere_is_read_as_written() {
        // A band 340 degrees wide, every edge shorter than half the globe.
        let band = polygon(
            &[
                (-170.0, -10.0),
                (0.0, -10.0),
                (170.0, -10.0),
                (170.0, 10.0),
                (0.0, 10.0),
                (-170.0, 10.0),
                (-170.0, -10.0),
            ],
            &[],
        );
        assert!(covers(&band, 90.0, 0.0));
        assert!(covers(&band, -90.0, 0.0));
        assert!(!covers(&band, 180.0, 0.0));
    }

    #[test]
    fn a_polygon_with_no_rings_holds_nothing() {
        assert!(!covers(&polygon(&[], &[]), 0.0, 0.0));
    }
}
