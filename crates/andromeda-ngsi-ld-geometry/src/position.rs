//! A single `GeoJSON` position.
//!
//! IETF RFC 7946 section 3.1.1 defines a position as an array of numbers whose
//! first two elements are longitude and latitude, with an optional third
//! element carrying altitude. It permits, but does not define, further
//! elements; NGSI-LD (ETSI GS CIM 009 v1.9.1 clause 4.7) adopts RFC 7946
//! wholesale and adds no meaning of its own for them, so a position with more
//! than three ordinates has no interpretation here and is rejected rather than
//! silently truncated.

use crate::dimension::GeometryDimension;
use geojson::Position as GeoJsonPosition;
use thiserror::Error;

/// Failures decoding a `GeoJSON` position array.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
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
}

/// A WGS84 / CRS84 position: longitude, latitude and an optional altitude.
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
    #[must_use]
    pub const fn new(longitude: f64, latitude: f64, altitude: Option<f64>) -> Position {
        Position { longitude, latitude, altitude }
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

impl TryFrom<&GeoJsonPosition> for Position {
    type Error = PositionError;

    fn try_from(position: &GeoJsonPosition) -> Result<Position, PositionError> {
        match position.as_slice() {
            [longitude, latitude] => Ok(Position::new(*longitude, *latitude, None)),
            [longitude, latitude, altitude] => Ok(Position::new(*longitude, *latitude, Some(*altitude))),
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
        let position = Position::try_from(&GeoJsonPosition::from([13.38, 52.52])).expect("two ordinates are a valid position");
        pretty_assertions::assert_eq!(position.longitude(), 13.38);
        pretty_assertions::assert_eq!(position.latitude(), 52.52);
        pretty_assertions::assert_eq!(position.altitude(), None);
        pretty_assertions::assert_eq!(position.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_three_ordinate_position_decodes_with_an_altitude() {
        let position = Position::try_from(&GeoJsonPosition::from([13.38, 52.52, 34.5])).expect("three ordinates are a valid position");
        pretty_assertions::assert_eq!(position.altitude(), Some(34.5));
        pretty_assertions::assert_eq!(position.dimension(), GeometryDimension::ThreeD);
    }

    #[test]
    fn a_one_ordinate_position_is_rejected_as_too_few_ordinates() {
        let error = Position::try_from(&GeoJsonPosition::from(vec![1.0])).expect_err("one ordinate is not a position");
        pretty_assertions::assert_eq!(error, PositionError::TooFewOrdinates { ordinates: 1 });
    }

    #[test]
    fn a_four_ordinate_position_is_rejected_as_too_many_ordinates() {
        let error = Position::try_from(&GeoJsonPosition::from([1.0, 2.0, 3.0, 4.0])).expect_err("four ordinates have no defined meaning");
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
    fn a_position_renders_back_to_the_arity_it_was_decoded_from() {
        let flat = Position::try_from(&GeoJsonPosition::from([1.0, 2.0])).expect("decodes");
        let tall = Position::try_from(&GeoJsonPosition::from([1.0, 2.0, 3.0])).expect("decodes");
        pretty_assertions::assert_eq!(flat.to_geojson().as_slice(), &[1.0, 2.0]);
        pretty_assertions::assert_eq!(tall.to_geojson().as_slice(), &[1.0, 2.0, 3.0]);
    }
}
