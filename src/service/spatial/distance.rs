//! Great-circle (Haversine) distance helpers shared between spatial indexes
//! and subscription matching.

use geo::algorithm::Centroid;

/// Mean Earth radius in metres (IUGG recommended value).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Compute the great-circle distance in metres between the centroids of two
/// geometries using the Haversine formula.
///
/// Zero-dimensional geometries (`Point`) trivially return their own
/// coordinates. For `LineString`/`Polygon`/collections the centroid is used,
/// matching the semantics used elsewhere for `georel=near` on non-point
/// geometries.
pub fn haversine_distance_m(a: &geo::Geometry<f64>, b: &geo::Geometry<f64>) -> f64 {
    let centroid_a = a.centroid().unwrap_or(geo::Point::new(0.0, 0.0));
    let centroid_b = b.centroid().unwrap_or(geo::Point::new(0.0, 0.0));
    haversine_point_distance_m(&centroid_a, &centroid_b)
}

/// Compute the great-circle distance in metres between two points.
pub fn haversine_point_distance_m(a: &geo::Point<f64>, b: &geo::Point<f64>) -> f64 {
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
        let p = geo::Geometry::Point(geo::Point::new(2.3522, 48.8566));
        let d = haversine_distance_m(&p, &p);
        assert!(d.abs() < 1e-6, "expected ~0, got {}", d);
    }

    #[test]
    fn haversine_known_distance_paris_london() {
        let paris = geo::Geometry::Point(geo::Point::new(2.3522, 48.8566));
        let london = geo::Geometry::Point(geo::Point::new(-0.1276, 51.5074));
        let d = haversine_distance_m(&paris, &london);
        // Known distance ~344 km; allow 1% tolerance.
        assert!((d - 344_000.0).abs() < 3_500.0, "expected ~344km, got {} m", d);
    }

    #[test]
    fn metres_to_degrees_sanity() {
        let d = metres_to_degrees_coarse(111_320.0);
        assert!((d - 1.0).abs() < 1e-6);
    }
}
