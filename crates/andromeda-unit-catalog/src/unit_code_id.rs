//! The persisted two-byte identity of a unit code.

use derive_more::Display;
use thiserror::Error;

/// The persisted identity of one registered unit code.
///
/// The catalog is a few thousand rows and grows by a handful per revision, so a
/// signed two-byte key is the storage the column needs and nothing more. The
/// identifier is assigned once by the registry and never reassigned: it is
/// written into every attribute row that carries a unit, and reusing a number
/// would silently reinterpret history.
///
/// Zero is not an identity — the absence of a unit is the absence of the
/// column's value, never a sentinel row — and the upper bound is the signed
/// two-byte ceiling the storage type imposes.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct UnitCodeId(i16);

impl UnitCodeId {
    /// Return the in-memory representation.
    #[must_use]
    pub const fn get(self) -> i16 {
        self.0
    }

    /// Return the identity following this one, when one is representable.
    #[must_use]
    pub const fn next(self) -> Option<UnitCodeId> {
        match self.0.checked_add(1) {
            Some(successor) => Some(UnitCodeId(successor)),
            None => None,
        }
    }

    /// Return the first identity the registry assigns.
    #[must_use]
    pub const fn first() -> UnitCodeId {
        UnitCodeId(1)
    }
}

impl TryFrom<i16> for UnitCodeId {
    type Error = UnitCodeIdOutOfRange;

    fn try_from(stored: i16) -> Result<UnitCodeId, UnitCodeIdOutOfRange> {
        if stored > 0 { Ok(UnitCodeId(stored)) } else { Err(UnitCodeIdOutOfRange) }
    }
}

impl TryFrom<i32> for UnitCodeId {
    type Error = UnitCodeIdOutOfRange;

    fn try_from(stored: i32) -> Result<UnitCodeId, UnitCodeIdOutOfRange> {
        i16::try_from(stored).map_err(|_| UnitCodeIdOutOfRange).and_then(UnitCodeId::try_from)
    }
}

/// A unit-code identity was outside the range the catalog assigns.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a unit-code identity is a positive number below 32768")]
pub struct UnitCodeIdOutOfRange;

#[cfg(test)]
mod tests {
    use crate::unit_code_id::{UnitCodeId, UnitCodeIdOutOfRange};

    #[test]
    fn the_registry_starts_at_one() {
        assert_eq!(UnitCodeId::first().get(), 1);
    }

    #[test]
    fn a_stored_identity_round_trips() {
        assert_eq!(UnitCodeId::try_from(1756_i16).map(UnitCodeId::get), Ok(1756));
        assert_eq!(UnitCodeId::try_from(1756_i32).map(UnitCodeId::get), Ok(1756));
    }

    #[test]
    fn zero_and_negative_identities_are_refused() {
        assert_eq!(UnitCodeId::try_from(0_i16), Err(UnitCodeIdOutOfRange));
        assert_eq!(UnitCodeId::try_from(-1_i16), Err(UnitCodeIdOutOfRange));
    }

    #[test]
    fn an_identity_beyond_the_two_byte_ceiling_is_refused() {
        assert_eq!(UnitCodeId::try_from(32_768_i32), Err(UnitCodeIdOutOfRange));
    }

    #[test]
    fn successors_run_out_at_the_two_byte_ceiling() {
        assert_eq!(UnitCodeId::first().next(), UnitCodeId::try_from(2_i16).ok());
        assert_eq!(UnitCodeId::try_from(i16::MAX).ok().and_then(UnitCodeId::next), None);
    }
}
