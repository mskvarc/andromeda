//! A single `GeoJSON` position.
//!
//! IETF RFC 7946 section 3.1.1 defines a position as an array of numbers whose
//! first two elements are longitude and latitude, with an optional third
//! element carrying altitude. It permits, but does not define, further
//! elements; NGSI-LD (ETSI GS CIM 009 v1.9.1 clause 4.7) adopts RFC 7946
//! wholesale and adds no meaning of its own for them, so a position with more
//! than three ordinates has no interpretation here and is rejected rather than
//! silently truncated.
//!
//! RFC 7946 section 4 places every position in WGS 84 (`CRS84`), with
//! longitude and latitude in decimal degrees, so a longitude outside [-180, 180]
//! or a latitude outside [-90, 90] names no position at all and is rejected
//! too. Clause 4.7.2 accepts a `GeoProperty` geometry only when it represents
//! "a valid Geometry" under RFC 7946, and clause 4.10 holds a reference
//! geometry to RFC 7946 section 3.1.1. Left in, such a value would mean three
//! different things: the planar georels read it as given, `PostGIS` geography
//! silently wraps or reflects it for `near`, and the in-memory distance turns
//! a latitude past a pole into a wrong or NaN distance. Altitude is height
//! above or below the ellipsoid and has no such bound.

use crate::dimension::GeometryDimension;
use geojson::Position as GeoJsonPosition;
use std::ops::RangeInclusive;
use thiserror::Error;

/// The CRS84 longitude axis, in decimal degrees. Both ends name the antimeridian
/// and both are valid, as RFC 7946 section 3.1.9's own examples use them.
const LONGITUDE_DEGREES: RangeInclusive<f64> = -180.0..=180.0;

/// The CRS84 latitude axis, in decimal degrees, pole to pole.
const LATITUDE_DEGREES: RangeInclusive<f64> = -90.0..=90.0;

/// Failures decoding a `GeoJSON` position array.
#[derive(Debug, Error, Clone, Copy, PartialEq)]
pub enum PositionError {
    /// Fewer than the two ordinates RFC 7946 section 3.1.1 requires
    /// ("a position is an array of numbers[; t]here MUST be two or more
    /// elements").
    #[error("GeoJSON position has {ordinates} ordinates; RFC 7946 allows exactly 2 (longitude, latitude) or 3 (longitude, latitude, altitude)")]
    TooFewOrdinates {
        /// The number of ordinates the position actually carried.
        ordinates: usize,
    },
    /// More than longitude, latitude and altitude. RFC 7946 section 3.1.1
    /// allows additional elements but assigns them no meaning, and NGSI-LD
    /// assigns them none either, so they cannot be carried without inventing
    /// semantics the specification does not define.
    #[error("GeoJSON position has {ordinates} ordinates; RFC 7946 allows exactly 2 (longitude, latitude) or 3 (longitude, latitude, altitude)")]
    TooManyOrdinates {
        /// The number of ordinates the position actually carried.
        ordinates: usize,
    },
    /// A position member that is not a number. RFC 7946 section 3.1.1 defines a
    /// position as "an array of numbers", so a string or any other JSON scalar
    /// has no ordinate reading and cannot be stored or matched against.
    #[error("GeoJSON position has a non-numeric ordinate at index {index}; RFC 7946 requires every ordinate to be a number")]
    NonNumericOrdinate {
        /// Zero-based index of the offending member within the position array.
        index: usize,
    },
    /// A longitude outside the CRS84 axis RFC 7946 section 4 fixes.
    #[error(
        "GeoJSON position longitude {longitude} is outside [-180, 180]; RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees"
    )]
    LongitudeOutOfRange {
        /// The longitude the position carried.
        longitude: f64,
    },
    /// A latitude outside the CRS84 axis RFC 7946 section 4 fixes, pole to
    /// pole. Section 5.3 holds even a bounding box to it ("MUST NOT use latitude
    /// values greater than 90 or less than -90 to imply an extent that is not a
    /// spherical cap").
    #[error(
        "GeoJSON position latitude {latitude} is outside [-90, 90]; RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees"
    )]
    LatitudeOutOfRange {
        /// The latitude the position carried.
        latitude: f64,
    },
    /// An altitude that is not a finite number of metres.
    #[error("GeoJSON position altitude {altitude} is not a finite number of metres")]
    NonFiniteAltitude {
        /// The altitude the position carried.
        altitude: f64,
    },
}

/// A WGS84 / CRS84 position: longitude, latitude and an optional altitude.
///
/// Every `Position` lies on the CRS84 axes: the only constructors check the
/// ranges, so no geometry the broker holds, whatever path built it, can carry
/// an ordinate RFC 7946 section 4 gives no meaning.
///
/// Equality is structural over the ordinates, altitude included, so two
/// positions that differ only in altitude are different positions. There is
/// deliberately no `Eq`/`Hash`: the ordinates are `f64`, over which neither is
/// meaningful.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Easting in decimal degrees, the first ordinate of the `GeoJSON` array.
    longitude: f64,
    /// Northing in decimal degrees, the second ordinate of the `GeoJSON` array.
    latitude: f64,
    /// Height in metres above the WGS84 ellipsoid, the optional third ordinate.
    altitude: Option<f64>,
}

impl Position {
    /// Build a position from its ordinates.
    ///
    /// # Errors
    ///
    /// Returns [`PositionError::LongitudeOutOfRange`] for a longitude outside
    /// [-180, 180], [`PositionError::LatitudeOutOfRange`] for a latitude outside
    /// [-90, 90], checked in that order, and [`PositionError::NonFiniteAltitude`]
    /// for an altitude that is not finite. A non-finite longitude or latitude
    /// is outside its axis.
    pub fn try_new(longitude: f64, latitude: f64, altitude: Option<f64>) -> Result<Position, PositionError> {
        if !LONGITUDE_DEGREES.contains(&longitude) {
            return Err(PositionError::LongitudeOutOfRange { longitude });
        }
        if !LATITUDE_DEGREES.contains(&latitude) {
            return Err(PositionError::LatitudeOutOfRange { latitude });
        }
        match altitude {
            Some(altitude) if !altitude.is_finite() => Err(PositionError::NonFiniteAltitude { altitude }),
            Some(_) | None => Ok(Position { longitude, latitude, altitude }),
        }
    }

    /// Easting in decimal degrees.
    #[must_use]
    pub const fn longitude(&self) -> f64 {
        self.longitude
    }

    /// Northing in decimal degrees.
    #[must_use]
    pub const fn latitude(&self) -> f64 {
        self.latitude
    }

    /// Height in metres above the WGS84 ellipsoid, or `None` for a position
    /// that was received without a third ordinate.
    #[must_use]
    pub const fn altitude(&self) -> Option<f64> {
        self.altitude
    }

    /// Whether this position carries an altitude.
    #[must_use]
    pub const fn dimension(&self) -> GeometryDimension {
        match self.altitude {
            Some(_) => GeometryDimension::ThreeD,
            None => GeometryDimension::TwoD,
        }
    }

    /// Render as a `GeoJSON` position array.
    ///
    /// The arity received is the arity produced: a position decoded from two
    /// ordinates renders back to two, one decoded from three renders back to
    /// three.
    #[must_use]
    pub fn to_geojson(&self) -> GeoJsonPosition {
        match self.altitude {
            Some(altitude) => GeoJsonPosition::from([self.longitude, self.latitude, altitude]),
            None => GeoJsonPosition::from([self.longitude, self.latitude]),
        }
    }
}

impl TryFrom<&[f64]> for Position {
    type Error = PositionError;

    fn try_from(ordinates: &[f64]) -> Result<Position, PositionError> {
        match ordinates {
            [longitude, latitude] => Position::try_new(*longitude, *latitude, None),
            [longitude, latitude, altitude] => Position::try_new(*longitude, *latitude, Some(*altitude)),
            ordinates if ordinates.len() < 2 => Err(PositionError::TooFewOrdinates { ordinates: ordinates.len() }),
            ordinates => Err(PositionError::TooManyOrdinates { ordinates: ordinates.len() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_ordinate_position_decodes_without_an_altitude() {
        let position = Position::try_from(&[13.38, 52.52][..]).expect("two ordinates are a valid position");
        // The ordinates are carried, not computed, so the decoded values are
        // the very bits the `GeoJSON` position held.
        assert!(
            (position.longitude() - 13.38).abs() < f64::EPSILON,
            "longitude is carried unchanged: {}",
            position.longitude()
        );
        assert!(
            (position.latitude() - 52.52).abs() < f64::EPSILON,
            "latitude is carried unchanged: {}",
            position.latitude()
        );
        pretty_assertions::assert_eq!(position.altitude(), None);
        pretty_assertions::assert_eq!(position.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_three_ordinate_position_decodes_with_an_altitude() {
        let position = Position::try_from(&[13.38, 52.52, 34.5][..]).expect("three ordinates are a valid position");
        let altitude = position.altitude().expect("a three-ordinate position carries an altitude");
        assert!((altitude - 34.5).abs() < f64::EPSILON, "altitude is carried unchanged: {altitude}");
        pretty_assertions::assert_eq!(position.dimension(), GeometryDimension::ThreeD);
    }

    #[test]
    fn a_one_ordinate_position_is_rejected_as_too_few_ordinates() {
        let error = Position::try_from(&[1.0][..]).expect_err("one ordinate is not a position");
        pretty_assertions::assert_eq!(error, PositionError::TooFewOrdinates { ordinates: 1 });
    }

    #[test]
    fn a_four_ordinate_position_is_rejected_as_too_many_ordinates() {
        let error = Position::try_from(&[1.0, 2.0, 3.0, 4.0][..]).expect_err("four ordinates have no defined meaning");
        pretty_assertions::assert_eq!(error, PositionError::TooManyOrdinates { ordinates: 4 });
    }

    #[test]
    fn an_arity_failure_states_the_ordinate_count_and_the_rfc_7946_rule() {
        // This wording is the diagnostic every write-side and query-side caller
        // prefixes with its own locus to build the BadRequestData detail that
        // ETSI GS CIM 009 v1.9.1 clause 5.5.4 requires, so it is contractual
        // rather than incidental.
        pretty_assertions::assert_eq!(
            PositionError::TooManyOrdinates { ordinates: 4 }.to_string(),
            "GeoJSON position has 4 ordinates; RFC 7946 allows exactly 2 (longitude, latitude) or 3 (longitude, latitude, altitude)"
        );
        pretty_assertions::assert_eq!(
            PositionError::TooFewOrdinates { ordinates: 1 }.to_string(),
            "GeoJSON position has 1 ordinates; RFC 7946 allows exactly 2 (longitude, latitude) or 3 (longitude, latitude, altitude)"
        );
    }

    #[test]
    fn the_ends_of_both_crs84_axes_are_positions() {
        for ordinates in [[180.0, 90.0], [-180.0, -90.0], [0.0, 0.0], [-0.0, -0.0]] {
            let position = Position::try_from(&ordinates[..]).expect("an axis end is a position");
            pretty_assertions::assert_eq!(position.to_geojson().as_slice(), &ordinates);
        }
    }

    #[test]
    fn a_longitude_past_the_antimeridian_is_rejected_with_its_value() {
        for longitude in [180.000_001, -180.000_001, 360.0] {
            let error = Position::try_from(&[longitude, 0.0][..]).expect_err("CRS84 longitude ends at 180");
            pretty_assertions::assert_eq!(error, PositionError::LongitudeOutOfRange { longitude });
        }
    }

    #[test]
    fn a_latitude_past_a_pole_is_rejected_with_its_value() {
        for latitude in [90.000_001, -90.000_001] {
            let error = Position::try_from(&[0.0, latitude][..]).expect_err("CRS84 latitude ends at the poles");
            pretty_assertions::assert_eq!(error, PositionError::LatitudeOutOfRange { latitude });
        }
    }

    #[test]
    fn a_position_out_of_range_on_both_axes_reports_its_longitude() {
        let error = Position::try_from(&[1000.0, 2000.0][..]).expect_err("neither ordinate is on its axis");
        pretty_assertions::assert_eq!(error, PositionError::LongitudeOutOfRange { longitude: 1000.0 });
    }

    #[test]
    fn an_arity_failure_is_reported_before_any_range() {
        let error = Position::try_from(&[1000.0][..]).expect_err("one ordinate is not a position");
        pretty_assertions::assert_eq!(error, PositionError::TooFewOrdinates { ordinates: 1 });
    }

    #[test]
    fn altitude_has_no_range_but_must_be_finite() {
        for altitude in [-11_000.0, 400_000.0] {
            let position = Position::try_new(14.5, 46.05, Some(altitude)).expect("any finite altitude is a height above or below the ellipsoid");
            pretty_assertions::assert_eq!(position.altitude(), Some(altitude));
        }
        let error = Position::try_new(14.5, 46.05, Some(f64::INFINITY)).expect_err("an infinite altitude is no height");
        pretty_assertions::assert_eq!(error, PositionError::NonFiniteAltitude { altitude: f64::INFINITY });
    }

    #[test]
    fn a_non_finite_longitude_or_latitude_is_outside_its_axis() {
        assert!(matches!(Position::try_new(f64::NAN, 0.0, None), Err(PositionError::LongitudeOutOfRange { .. })));
        assert!(matches!(
            Position::try_new(0.0, f64::NEG_INFINITY, None),
            Err(PositionError::LatitudeOutOfRange { .. })
        ));
    }

    #[test]
    fn a_range_failure_names_the_ordinate_its_value_and_the_rfc_7946_rule() {
        pretty_assertions::assert_eq!(
            PositionError::LatitudeOutOfRange { latitude: 100.0 }.to_string(),
            "GeoJSON position latitude 100 is outside [-90, 90]; RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees"
        );
        pretty_assertions::assert_eq!(
            PositionError::LongitudeOutOfRange { longitude: -181.5 }.to_string(),
            "GeoJSON position longitude -181.5 is outside [-180, 180]; RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees"
        );
    }

    #[test]
    fn a_position_renders_back_to_the_arity_it_was_decoded_from() {
        let flat = Position::try_from(&[1.0, 2.0][..]).expect("decodes");
        let tall = Position::try_from(&[1.0, 2.0, 3.0][..]).expect("decodes");
        pretty_assertions::assert_eq!(flat.to_geojson().as_slice(), &[1.0, 2.0]);
        pretty_assertions::assert_eq!(tall.to_geojson().as_slice(), &[1.0, 2.0, 3.0]);
    }
}
