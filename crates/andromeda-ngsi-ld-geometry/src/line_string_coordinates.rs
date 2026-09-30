//! The positions of one `GeoJSON` `LineString`.
//!
//! IETF RFC 7946 section 3.1.4 makes the `coordinates` of a `LineString` "an
//! array of two or more positions", and a `MultiLineString` an array of such
//! arrays. A single position traces no line, so it is refused where the line
//! is built, the same way a position off the CRS84 axes is refused.

use crate::position::Position;
use thiserror::Error;

/// The fewest positions a `LineString` can hold (RFC 7946 section 3.1.4).
const MINIMUM_POSITIONS: usize = 2;

/// Why a list of positions is not a `LineString`.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum LineStringError {
    /// The line holds fewer than two positions.
    #[error("GeoJSON LineString has {positions} of the two or more positions RFC 7946 section 3.1.4 requires")]
    TooFewPositions {
        /// How many positions the line holds.
        positions: usize,
    },
}

/// The two or more positions of a `LineString`, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct LineStringCoordinates(Vec<Position>);

impl LineStringCoordinates {
    /// Build a line from its positions.
    ///
    /// # Errors
    ///
    /// Returns [`LineStringError::TooFewPositions`] for fewer than two
    /// positions.
    pub fn try_new(positions: Vec<Position>) -> Result<LineStringCoordinates, LineStringError> {
        if positions.len() < MINIMUM_POSITIONS {
            return Err(LineStringError::TooFewPositions { positions: positions.len() });
        }
        Ok(LineStringCoordinates(positions))
    }

    /// The positions of the line, in order.
    #[must_use]
    pub fn positions(&self) -> &[Position] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        line_string_coordinates::{LineStringCoordinates, LineStringError},
        position::Position,
    };

    fn flat(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    #[test]
    fn two_positions_make_a_line() {
        let positions = vec![flat(0.0, 0.0), flat(1.0, 1.0)];
        // clone: the line consumes the positions it is compared against
        let line = LineStringCoordinates::try_new(positions.clone()).expect("two positions trace a line");
        pretty_assertions::assert_eq!(line.positions(), positions.as_slice());
    }

    #[test]
    fn fewer_than_two_positions_trace_no_line() {
        pretty_assertions::assert_eq!(
            LineStringCoordinates::try_new(vec![flat(0.0, 0.0)]),
            Err(LineStringError::TooFewPositions { positions: 1 })
        );
        pretty_assertions::assert_eq!(
            LineStringCoordinates::try_new(Vec::new()),
            Err(LineStringError::TooFewPositions { positions: 0 })
        );
    }

    #[test]
    fn the_refusal_names_the_rule() {
        pretty_assertions::assert_eq!(
            LineStringError::TooFewPositions { positions: 1 }.to_string(),
            "GeoJSON LineString has 1 of the two or more positions RFC 7946 section 3.1.4 requires"
        );
    }
}
