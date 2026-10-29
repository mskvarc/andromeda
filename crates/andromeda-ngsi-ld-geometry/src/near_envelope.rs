//! The region a `near;maxDistance` can reach.
//!
//! An index or a bounding-box prefilter may hand the exact predicate more
//! candidates than match, never fewer. The envelope here is built to cover
//! every point within the distance of the reference on the WGS 84 spheroid,
//! which is where clause 4.10 `near` is measured (see [`crate::proximity`]),
//! and it wraps across the antimeridian rather than stopping at it.

use crate::{great_circle_reach::REACH_TOLERANCE_DEGREES, positive_distance::PositiveDistance};
use geo::{Coord, Rect};
use std::slice;

/// The smallest radius of curvature of the WGS 84 ellipsoid, in metres: the
/// meridional radius at the equator, `b^2 / a` with `a = 6 378 137` and
/// `b = 6 356 752.314 245`.
///
/// Along any geodesic, latitude changes at most `|cos(azimuth)| / M(latitude)`
/// radians per metre and longitude at most
/// `|sin(azimuth)| / (N(latitude) cos(latitude))`, where `M` is the meridional
/// and `N` the prime-vertical radius of curvature. Both radii are at least this
/// value, so a distance divided by it bounds the angle a geodesic of that
/// length can turn through on either axis.
const WGS84_MIN_RADIUS_OF_CURVATURE_M: f64 = 6_335_439.327;

/// Every point within a distance of a reference rectangle, as one or two
/// longitude/latitude rectangles that together cover it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NearEnvelope {
    /// One rectangle inside [-180, 180]: the whole longitude range when a pole
    /// is within reach or the reach wraps the globe.
    Single(Rect<f64>),
    /// The region wraps across the antimeridian: the part ending at +180 and
    /// the part starting at -180.
    AcrossAntimeridian([Rect<f64>; 2]),
}

impl NearEnvelope {
    /// The envelope of every point within `distance` of anything inside
    /// `reference`.
    ///
    /// The latitude reach is the distance over
    /// [`WGS84_MIN_RADIUS_OF_CURVATURE_M`], and the longitude reach that same
    /// angle widened by the cosine of the reached latitude furthest from the
    /// equator. Both are upper bounds, so the envelope covers rather than
    /// approximates; it overshoots by about 0.6 % plus the widening. Each side
    /// is pushed out by a further [`REACH_TOLERANCE_DEGREES`], the most a
    /// stored box may fall short of its geometry's great-circle reach.
    #[must_use]
    pub fn around(reference: Rect<f64>, distance: PositiveDistance) -> NearEnvelope {
        NearEnvelope::around_metres(reference, distance.metres())
    }

    /// [`NearEnvelope::around`] for a distance already known to be a
    /// non-negative number of metres, as a search radius derived from one is.
    pub(crate) fn around_metres(reference: Rect<f64>, metres: f64) -> NearEnvelope {
        let reach = (metres / WGS84_MIN_RADIUS_OF_CURVATURE_M).to_degrees();
        let south = (reference.min().y - reach - REACH_TOLERANCE_DEGREES).max(-90.0);
        let north = (reference.max().y + reach + REACH_TOLERANCE_DEGREES).min(90.0);
        // Every meridian meets at a pole, so once one is within reach so is
        // every longitude.
        if south <= -90.0 || north >= 90.0 {
            return NearEnvelope::Single(rect(-180.0, south, 180.0, north));
        }
        let widening = reach / north.abs().max(south.abs()).to_radians().cos() + REACH_TOLERANCE_DEGREES;
        let west = reference.min().x - widening;
        let east = reference.max().x + widening;
        if east - west >= 360.0 {
            NearEnvelope::Single(rect(-180.0, south, 180.0, north))
        } else if west < -180.0 {
            NearEnvelope::AcrossAntimeridian([rect(west + 360.0, south, 180.0, north), rect(-180.0, south, east, north)])
        } else if east > 180.0 {
            NearEnvelope::AcrossAntimeridian([rect(west, south, 180.0, north), rect(-180.0, south, east - 360.0, north)])
        } else {
            NearEnvelope::Single(rect(west, south, east, north))
        }
    }

    /// The rectangles that together cover the region.
    #[must_use]
    pub const fn rects(&self) -> &[Rect<f64>] {
        match self {
            NearEnvelope::Single(rect) => slice::from_ref(rect),
            NearEnvelope::AcrossAntimeridian(rects) => rects,
        }
    }

    /// One rectangle covering the whole region, for a consumer that holds a
    /// single box per object: a region that wraps across the antimeridian
    /// becomes the full longitude band.
    #[must_use]
    pub fn covering_rect(&self) -> Rect<f64> {
        match self {
            NearEnvelope::Single(rect) => *rect,
            NearEnvelope::AcrossAntimeridian([east, _]) => rect(-180.0, east.min().y, 180.0, east.max().y),
        }
    }
}

/// The rectangle between two corners.
fn rect(west: f64, south: f64, east: f64, north: f64) -> Rect<f64> {
    Rect::new(Coord { x: west, y: south }, Coord { x: east, y: north })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{great_circle_reach::REACH_TOLERANCE_DEGREES, near_envelope::NearEnvelope, positive_distance::PositiveDistance};
    use geo::{Destination, Geodesic, Point, Rect};
    use std::str::FromStr;

    fn metres(value: &str) -> PositiveDistance {
        PositiveDistance::from_str(value).expect("positive distance")
    }

    fn at(longitude: f64, latitude: f64) -> Rect<f64> {
        Rect::new((longitude, latitude), (longitude, latitude))
    }

    /// Whether one of the envelope's rectangles holds the point, its longitude
    /// read in [-180, 180].
    fn covers(envelope: &NearEnvelope, point: Point<f64>) -> bool {
        let longitude = (point.x() + 540.0).rem_euclid(360.0) - 180.0;
        envelope
            .rects()
            .iter()
            .any(|rect| rect.min().x <= longitude && longitude <= rect.max().x && rect.min().y <= point.y() && point.y() <= rect.max().y)
    }

    /// The bearings, one degree apart, whose spheroidal destination at
    /// `distance` from `centre` the envelope misses.
    fn missed_bearings(envelope: &NearEnvelope, centre: Point<f64>, distance: f64) -> Vec<u16> {
        (0..360_u16)
            .filter(|&bearing| !covers(envelope, Geodesic.destination(centre, f64::from(bearing), distance)))
            .collect()
    }

    #[test]
    fn the_envelope_covers_every_spheroidal_destination_at_the_equator() {
        // On the spheroid 5 000 m due north of the equator is latitude
        // 0.045 218; a box grown on the sphere stops at 0.044 966.
        let envelope = NearEnvelope::around(at(0.0, 0.0), metres("5000"));
        assert!(matches!(envelope, NearEnvelope::Single(_)), "{envelope:?}");
        pretty_assertions::assert_eq!(missed_bearings(&envelope, Point::new(0.0, 0.0), 5000.0), Vec::<u16>::new());
    }

    #[test]
    fn the_envelope_covers_the_east_west_extent_at_high_latitude() {
        let envelope = NearEnvelope::around(at(0.0, 60.0), metres("300000"));
        pretty_assertions::assert_eq!(missed_bearings(&envelope, Point::new(0.0, 60.0), 300_000.0), Vec::<u16>::new());
    }

    #[test]
    fn the_envelope_covers_every_destination_over_a_sweep_of_latitudes_and_distances() {
        for latitude in [-89.0, -80.0, -60.0, -30.0, 0.0, 30.0, 53.0, 60.0, 80.0, 85.0, 89.0] {
            for longitude in [-179.99, -100.0, 0.0, 120.0, 179.99] {
                for distance in [10.0, 150.0, 5000.0, 150_000.0, 1_000_000.0, 3_000_000.0] {
                    let envelope = NearEnvelope::around(at(longitude, latitude), metres(&distance.to_string()));
                    pretty_assertions::assert_eq!(
                        missed_bearings(&envelope, Point::new(longitude, latitude), distance),
                        Vec::<u16>::new(),
                        "({longitude}, {latitude}) at {distance} m"
                    );
                }
            }
        }
    }

    #[test]
    fn the_envelope_of_a_short_distance_stays_tight_enough_to_prune() {
        // 150 m at 53 degrees north: the ideal width is twice the reach over
        // the length of a degree of longitude there.
        let envelope = NearEnvelope::around(at(8.8072, 53.0758), metres("150"));
        pretty_assertions::assert_eq!(missed_bearings(&envelope, Point::new(8.8072, 53.0758), 150.0), Vec::<u16>::new());
        let width = envelope.covering_rect().width();
        let ideal = 2.0 * 150.0 / (111_320.0 * 53.0758_f64.to_radians().cos());
        assert!(width < ideal * 1.02, "width {width} against ideal {ideal}");
    }

    #[test]
    fn a_polygon_reference_grows_from_every_corner_of_its_bounds() {
        let reference = Rect::new((8.80, 53.07), (8.81, 53.08));
        let envelope = NearEnvelope::around(reference, metres("150"));
        let NearEnvelope::Single(rect) = envelope else {
            panic!("a reference far from the antimeridian stays one rectangle: {envelope:?}");
        };
        assert!(rect.min().x < 8.80 && rect.min().y < 53.07 && rect.max().x > 8.81 && rect.max().y > 53.08);
        for (longitude, latitude) in [(8.80, 53.07), (8.81, 53.07), (8.81, 53.08), (8.80, 53.08)] {
            pretty_assertions::assert_eq!(missed_bearings(&envelope, Point::new(longitude, latitude), 150.0), Vec::<u16>::new());
        }
    }

    #[test]
    fn an_envelope_crossing_the_antimeridian_splits_in_two() {
        let envelope = NearEnvelope::around(at(179.999, -17.0), metres("300"));
        let NearEnvelope::AcrossAntimeridian([east, west]) = envelope else {
            panic!("expected a split envelope: {envelope:?}");
        };
        pretty_assertions::assert_eq!((east.max().x, west.min().x), (180.0, -180.0));
        assert!(west.max().x > -180.0, "the far side is reached");
        pretty_assertions::assert_eq!(missed_bearings(&envelope, Point::new(179.999, -17.0), 300.0), Vec::<u16>::new());
        pretty_assertions::assert_eq!(envelope.rects().len(), 2);
    }

    #[test]
    fn an_envelope_reaching_a_pole_spans_every_longitude() {
        let envelope = NearEnvelope::around(at(0.0, 89.9), metres("25000"));
        let NearEnvelope::Single(rect) = envelope else {
            panic!("expected one rectangle: {envelope:?}");
        };
        pretty_assertions::assert_eq!((rect.min().x, rect.max().x, rect.max().y), (-180.0, 180.0, 90.0));
        pretty_assertions::assert_eq!(envelope.rects().len(), 1);
    }

    /// A stored box may fall short of its geometry's great-circle reach by up
    /// to the tolerance, so even an envelope of no distance covers a point
    /// that far off the reference on either axis.
    #[test]
    fn the_envelope_at_distance_zero_covers_a_point_one_tolerance_away() {
        let envelope = NearEnvelope::around_metres(at(10.0, 50.0), 0.0);
        for (longitude, latitude) in [
            (10.0 + REACH_TOLERANCE_DEGREES, 50.0),
            (10.0 - REACH_TOLERANCE_DEGREES, 50.0),
            (10.0, 50.0 + REACH_TOLERANCE_DEGREES),
            (10.0, 50.0 - REACH_TOLERANCE_DEGREES),
        ] {
            assert!(
                covers(&envelope, Point::new(longitude, latitude)),
                "({longitude}, {latitude}) outside {envelope:?}"
            );
        }
    }

    #[test]
    fn an_envelope_wider_than_the_globe_spans_everything() {
        let envelope = NearEnvelope::around(at(0.0, 0.0), metres("21000000"));
        pretty_assertions::assert_eq!(envelope, NearEnvelope::Single(Rect::new((-180.0, -90.0), (180.0, 90.0))));
    }

    #[test]
    fn the_covering_rect_of_a_split_envelope_is_the_full_longitude_band() {
        let envelope = NearEnvelope::around(at(179.999, -17.0), metres("300"));
        let covering = envelope.covering_rect();
        let [east, _] = envelope.rects() else {
            panic!("expected a split envelope: {envelope:?}");
        };
        pretty_assertions::assert_eq!(covering, Rect::new((-180.0, east.min().y), (180.0, east.max().y)));
    }
}
