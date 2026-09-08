//! Great-circle (Haversine) distance helpers shared between spatial indexes
//! and subscription matching.

use geo::{Destination, Geometry as GeoGeometry, Haversine, Point, algorithm::Centroid};

/// Mean Earth radius in metres (IUGG recommended value).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// The great-circle distance in metres between the centroids of two
/// geometries, or `None` when either has no centroid.
///
/// Zero-dimensional geometries (`Point`) trivially return their own
/// coordinates. For `LineString`/`Polygon`/collections the centroid is used,
/// matching the semantics used elsewhere for `georel=near` on non-point
/// geometries. An empty geometry has no centroid at all, and there is no
/// distance to anything from a geometry that is nowhere — this used to default
/// to latitude 0, longitude 0, which put such a geometry in the Gulf of Guinea
/// and let `near;maxDistance` match it.
#[must_use]
pub fn haversine_distance_m(a: &GeoGeometry<f64>, b: &GeoGeometry<f64>) -> Option<f64> {
    Some(haversine_point_distance_m(&a.centroid()?, &b.centroid()?))
}

/// Compute the great-circle distance in metres between two points.
#[must_use]
pub fn haversine_point_distance_m(a: &Point<f64>, b: &Point<f64>) -> f64 {
    let (lat1, lon1) = (a.y().to_radians(), a.x().to_radians());
    let (lat2, lon2) = (b.y().to_radians(), b.x().to_radians());

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * h.sqrt().asin();

    EARTH_RADIUS_M * c
}

/// Grow a `[lon, lat]` bounding box outward by `metres` in every direction,
/// following great-circle paths.
///
/// The north-south grow is near-constant, but the east-west grow widens with
/// latitude because a degree of longitude shrinks toward the poles. Rather than
/// hand-rolling a `cos(latitude)` factor, the four grown edges are read off
/// `geo`'s Haversine destination (bearings N/E/S/W), so the geodesy lives in the
/// library and stays correct at every latitude.
///
/// It is a deliberate over-estimate for R-tree pre-filtering of a `near` query:
/// the exact Haversine check downstream rejects the false positives it admits,
/// and no envelope can recover a false negative. The returned corners are
/// clamped so the box only ever widens — an edge that would wrap (past a pole or
/// the antimeridian) leaves that side unchanged, and the whole-partition rescue
/// in the subscription index still catches anything such a wrap would miss.
#[must_use]
pub fn grow_lonlat_bounds_metres(min: [f64; 2], max: [f64; 2], metres: f64) -> ([f64; 2], [f64; 2]) {
    // A degree of longitude is shortest — so the grow in degrees is widest — at
    // the latitude furthest from the equator; use it for both east and west.
    let widest_latitude = if max[1].abs() >= min[1].abs() { max[1] } else { min[1] };
    let north = Haversine.destination(Point::new(min[0], max[1]), 0.0, metres).y();
    let south = Haversine.destination(Point::new(min[0], min[1]), 180.0, metres).y();
    let east = Haversine.destination(Point::new(max[0], widest_latitude), 90.0, metres).x();
    let west = Haversine.destination(Point::new(min[0], widest_latitude), 270.0, metres).x();
    ([west.min(min[0]), south.min(min[1])], [east.max(max[0]), north.max(max[1])])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_zero_for_identical_points() {
        let p = GeoGeometry::Point(Point::new(2.3522, 48.8566));
        let d = haversine_distance_m(&p, &p).expect("a point is its own centroid");
        assert!(d.abs() < 1e-6, "expected ~0, got {d}");
    }

    #[test]
    fn haversine_known_distance_paris_london() {
        let paris = GeoGeometry::Point(Point::new(2.3522, 48.8566));
        let london = GeoGeometry::Point(Point::new(-0.1276, 51.5074));
        let d = haversine_distance_m(&paris, &london).expect("both points have centroids");
        // Known distance ~344 km; allow 1% tolerance.
        assert!((d - 344_000.0).abs() < 3_500.0, "expected ~344km, got {d} m");
    }

    #[test]
    fn a_geometry_with_no_centroid_is_at_no_distance_from_anything() {
        let empty = GeoGeometry::GeometryCollection(geo::GeometryCollection(Vec::new()));
        let paris = GeoGeometry::Point(Point::new(2.3522, 48.8566));
        assert_eq!(haversine_distance_m(&empty, &paris), None);
        assert_eq!(haversine_distance_m(&paris, &empty), None);
    }

    #[test]
    fn growing_a_point_box_at_the_equator_widens_it_by_about_a_degree_each_way() {
        // 111 320 m is ~1 degree at the equator on both axes (cos(0) = 1).
        let (min, max) = grow_lonlat_bounds_metres([0.0, 0.0], [0.0, 0.0], 111_320.0);
        let width = max[0] - min[0];
        let height = max[1] - min[1];
        assert!((width - 2.0).abs() < 1e-2, "expected ~2 degrees wide, got {width}");
        assert!((height - 2.0).abs() < 1e-2, "expected ~2 degrees tall, got {height}");
    }

    #[test]
    fn the_longitude_grow_widens_with_latitude() {
        // At 60N a degree of longitude is ~half the metres it is at the equator,
        // so the east-west grow must be ~twice the north-south grow. The old
        // latitude-only conversion is the bug this replaces: it grew both axes
        // equally and lost `near` matches to the east and west at high latitude.
        let (min, max) = grow_lonlat_bounds_metres([0.0, 60.0], [0.0, 60.0], 111_320.0);
        let width = max[0] - min[0];
        let height = max[1] - min[1];
        assert!(width > height * 1.9, "east-west grow {width} should be ~2x north-south grow {height} at 60N");
    }

    #[test]
    fn growing_a_box_only_ever_widens_it() {
        let (min, max) = grow_lonlat_bounds_metres([10.0, 20.0], [11.0, 21.0], 5_000.0);
        assert!(min[0] <= 10.0 && min[1] <= 20.0, "the lower corner must not shrink inward");
        assert!(max[0] >= 11.0 && max[1] >= 21.0, "the upper corner must not shrink inward");
    }
}
