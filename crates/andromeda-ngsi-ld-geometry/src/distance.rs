//! Great-circle (Haversine) distance helpers shared between spatial indexes
//! and subscription matching.

use geo::{Geometry as GeoGeometry, Point, algorithm::Centroid};

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
pub fn haversine_distance_m(a: &GeoGeometry<f64>, b: &GeoGeometry<f64>) -> Option<f64> {
    Some(haversine_point_distance_m(&a.centroid()?, &b.centroid()?))
}

/// Compute the great-circle distance in metres between two points.
pub fn haversine_point_distance_m(a: &Point<f64>, b: &Point<f64>) -> f64 {
    let (lat1, lon1) = (a.y().to_radians(), a.x().to_radians());
    let (lat2, lon2) = (b.y().to_radians(), b.x().to_radians());

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * h.sqrt().asin();

    EARTH_RADIUS_M * c
}

/// Approximate conversion of metres to degrees of latitude.
///
/// Degrees of latitude are ~111 320 m globally. Used to expand R-tree
/// envelopes for `near` queries by a coarse but safe overestimate.
pub fn metres_to_degrees_coarse(metres: f64) -> f64 {
    const METRES_PER_DEGREE: f64 = 111_320.0;
    metres / METRES_PER_DEGREE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_zero_for_identical_points() {
        let p = GeoGeometry::Point(Point::new(2.3522, 48.8566));
        let d = haversine_distance_m(&p, &p).expect("a point is its own centroid");
        assert!(d.abs() < 1e-6, "expected ~0, got {}", d);
    }

    #[test]
    fn haversine_known_distance_paris_london() {
        let paris = GeoGeometry::Point(Point::new(2.3522, 48.8566));
        let london = GeoGeometry::Point(Point::new(-0.1276, 51.5074));
        let d = haversine_distance_m(&paris, &london).expect("both points have centroids");
        // Known distance ~344 km; allow 1% tolerance.
        assert!((d - 344_000.0).abs() < 3_500.0, "expected ~344km, got {} m", d);
    }

    #[test]
    fn a_geometry_with_no_centroid_is_at_no_distance_from_anything() {
        let empty = GeoGeometry::GeometryCollection(geo::GeometryCollection(Vec::new()));
        let paris = GeoGeometry::Point(Point::new(2.3522, 48.8566));
        assert_eq!(haversine_distance_m(&empty, &paris), None);
        assert_eq!(haversine_distance_m(&paris, &empty), None);
    }

    #[test]
    fn metres_to_degrees_sanity() {
        let d = metres_to_degrees_coarse(111_320.0);
        assert!((d - 1.0).abs() < 1e-6);
    }
}
