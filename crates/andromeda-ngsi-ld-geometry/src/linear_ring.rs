//! One linear ring of a `GeoJSON` Polygon.
//!
//! IETF RFC 7946 section 3.1.6 defines a linear ring as "a closed `LineString`
//! with four or more positions" whose first and last positions "are
//! equivalent, and they MUST contain identical values". A ring that is open,
//! or too short to enclose anything, is not a ring at all, so it is refused
//! where it is built rather than repaired: the broker does not guess which
//! closing position a client meant, and what a client reads back is exactly
//! what it wrote.
//!
//! Winding order is deliberately not checked. Section 3.1.6 asks for the
//! right-hand rule but says parsers "SHOULD NOT reject Polygons that do not
//! follow" it, and clause 4.10 evaluates every predicate the same way either
//! way round.

use crate::position::Position;
use thiserror::Error;

/// The fewest positions a linear ring can hold: three distinct vertices and
/// the closing repeat of the first (RFC 7946 section 3.1.6).
const MINIMUM_POSITIONS: usize = 4;

/// Why a list of positions is not a linear ring.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum LinearRingError {
    /// The ring holds fewer than four positions.
    #[error("GeoJSON linear ring has {positions} of the four or more positions RFC 7946 section 3.1.6 requires")]
    TooFewPositions {
        /// How many positions the ring holds.
        positions: usize,
    },
    /// The ring's last position differs from its first.
    #[error("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first, altitude included")]
    Open,
}

/// A closed ring of four or more positions, the first repeated as the last.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearRing(Vec<Position>);

impl LinearRing {
    /// Build a ring from its positions, closing position included.
    ///
    /// # Errors
    ///
    /// Returns [`LinearRingError::TooFewPositions`] for a ring of fewer than
    /// four positions, and [`LinearRingError::Open`] for one whose last
    /// position does not repeat its first.
    pub fn try_new(positions: Vec<Position>) -> Result<LinearRing, LinearRingError> {
        if positions.len() < MINIMUM_POSITIONS {
            return Err(LinearRingError::TooFewPositions { positions: positions.len() });
        }
        if positions.first() != positions.last() {
            return Err(LinearRingError::Open);
        }
        Ok(LinearRing(positions))
    }

    /// The positions of the ring, the closing position included.
    #[must_use]
    pub fn positions(&self) -> &[Position] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        linear_ring::{LinearRing, LinearRingError},
        position::Position,
    };

    fn flat(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn tall(longitude: f64, latitude: f64, altitude: f64) -> Position {
        Position::try_new(longitude, latitude, Some(altitude)).expect("a test position lies on the CRS84 axes")
    }

    #[test]
    fn a_closed_ring_of_four_positions_is_a_ring() {
        let positions = vec![flat(0.0, 0.0), flat(1.0, 0.0), flat(1.0, 1.0), flat(0.0, 0.0)];
        // clone: the ring consumes the positions it is compared against
        let ring = LinearRing::try_new(positions.clone()).expect("closed, and four positions");
        pretty_assertions::assert_eq!(ring.positions(), positions.as_slice());
    }

    #[test]
    fn a_ring_of_three_positions_is_too_short_even_when_closed() {
        pretty_assertions::assert_eq!(
            LinearRing::try_new(vec![flat(0.0, 0.0), flat(1.0, 0.0), flat(0.0, 0.0)]),
            Err(LinearRingError::TooFewPositions { positions: 3 })
        );
        pretty_assertions::assert_eq!(LinearRing::try_new(Vec::new()), Err(LinearRingError::TooFewPositions { positions: 0 }));
    }

    #[test]
    fn a_ring_whose_last_position_differs_from_its_first_is_open() {
        pretty_assertions::assert_eq!(
            LinearRing::try_new(vec![flat(0.0, 0.0), flat(1.0, 0.0), flat(1.0, 1.0), flat(0.0, 1.0)]),
            Err(LinearRingError::Open)
        );
    }

    #[test]
    fn a_ring_whose_ends_differ_only_in_altitude_is_open() {
        // Section 3.1.6: the two ends "MUST contain identical values", and the
        // altitude is one of a position's values (section 3.1.1).
        pretty_assertions::assert_eq!(
            LinearRing::try_new(vec![tall(0.0, 0.0, 10.0), tall(1.0, 0.0, 10.0), tall(1.0, 1.0, 10.0), tall(0.0, 0.0, 20.0)]),
            Err(LinearRingError::Open)
        );
        pretty_assertions::assert_eq!(
            LinearRing::try_new(vec![tall(0.0, 0.0, 10.0), flat(1.0, 0.0), flat(1.0, 1.0), flat(0.0, 0.0)]),
            Err(LinearRingError::Open)
        );
    }

    #[test]
    fn a_clockwise_ring_is_a_ring() {
        // Section 3.1.6: parsers SHOULD NOT reject a ring against the
        // right-hand rule.
        assert!(LinearRing::try_new(vec![flat(0.0, 0.0), flat(0.0, 1.0), flat(1.0, 1.0), flat(1.0, 0.0), flat(0.0, 0.0)]).is_ok());
    }

    #[test]
    fn the_refusals_name_the_rule() {
        pretty_assertions::assert_eq!(
            LinearRingError::TooFewPositions { positions: 3 }.to_string(),
            "GeoJSON linear ring has 3 of the four or more positions RFC 7946 section 3.1.6 requires"
        );
        pretty_assertions::assert_eq!(
            LinearRingError::Open.to_string(),
            "GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first, altitude included"
        );
    }
}
