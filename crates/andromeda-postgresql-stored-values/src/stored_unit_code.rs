//! The stored form of a `unitCode`: the `cefact-units` identity of its code.
//!
//! The library assigns every code it lists a positive two-byte identity,
//! append-only and never reused, so the identity a release writes names the
//! same code for every later release. A stored value this build does not list
//! was written by a broker built against a newer library, and fails to decode
//! rather than being guessed at.

use cefact_units::UnitCode;
use thiserror::Error;

/// A stored unit-code identity names no code this build lists.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("the stored unit-code identity {stored} names no code this build lists")]
pub struct UnknownStoredUnitCode {
    /// The value the column held.
    pub stored: i16,
}

/// The value a `unit_code_id` column stores for `code`.
#[must_use]
pub const fn stored_unit_code(code: UnitCode) -> i16 {
    code.id().to_smallint()
}

/// The code a `unit_code_id` column value names.
///
/// # Errors
///
/// Returns [`UnknownStoredUnitCode`] when `stored` is no identity this build
/// lists: not positive, or assigned by a newer library release.
pub const fn unit_code_from_stored(stored: i16) -> Result<UnitCode, UnknownStoredUnitCode> {
    match UnitCode::from_smallint(stored) {
        Some(code) => Ok(code),
        None => Err(UnknownStoredUnitCode { stored }),
    }
}

#[cfg(test)]
mod tests {
    use crate::stored_unit_code::{UnknownStoredUnitCode, stored_unit_code, unit_code_from_stored};
    use cefact_units::UnitCode;

    #[test]
    fn every_code_round_trips_through_its_stored_value() {
        for &code in UnitCode::ALL {
            pretty_assertions::assert_eq!(unit_code_from_stored(stored_unit_code(code)), Ok(code), "{code}");
        }
    }

    /// The identities rows written before the library assigned them already
    /// hold, so no stored row changes meaning.
    #[test]
    fn stored_values_are_the_identities_existing_rows_hold() {
        pretty_assertions::assert_eq!(stored_unit_code(UnitCode::Kgm), 1121);
        pretty_assertions::assert_eq!(stored_unit_code(UnitCode::Cel), 367);
        pretty_assertions::assert_eq!(stored_unit_code(UnitCode::Mtr), 1407);
    }

    #[test]
    fn an_unknown_stored_value_fails_the_decode() {
        for stored in [0, -1, i16::MAX] {
            pretty_assertions::assert_eq!(unit_code_from_stored(stored), Err(UnknownStoredUnitCode { stored }), "{stored}");
        }
    }
}
