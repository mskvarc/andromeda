//! The 1-based ordinal of a retry, which is also the number of attempts lost
//! before it.
//!
//! A run that has not failed yet has no retry number, so zero is not a value of
//! the type: a persisted attempt count is converted through [`TryFrom`], and a
//! count that is zero or negative is refused before it can reach a schedule.

use derive_more::{Display, From};
use std::num::NonZeroU32;
use thiserror::Error;

/// The ordinal of one retry: retry `n` follows `n` lost attempts.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
pub struct RetryNumber(NonZeroU32);

impl RetryNumber {
    /// The retry that follows the first lost attempt.
    pub const FIRST: RetryNumber = RetryNumber(NonZeroU32::MIN);

    /// The retry at ordinal `ordinal`.
    #[must_use]
    pub const fn new(ordinal: NonZeroU32) -> RetryNumber {
        RetryNumber(ordinal)
    }

    /// The retry that follows this one; the last representable retry is its
    /// own successor.
    #[must_use]
    pub const fn next(self) -> RetryNumber {
        RetryNumber(self.0.saturating_add(1))
    }

    /// The ordinal as a plain count, at least one.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// A count that names no retry.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a retry number is at least one, not {0}")]
pub struct RetryNumberError(i64);

impl TryFrom<u32> for RetryNumber {
    type Error = RetryNumberError;

    fn try_from(ordinal: u32) -> Result<RetryNumber, RetryNumberError> {
        NonZeroU32::new(ordinal).map(RetryNumber).ok_or(RetryNumberError(i64::from(ordinal)))
    }
}

impl TryFrom<i32> for RetryNumber {
    type Error = RetryNumberError;

    fn try_from(ordinal: i32) -> Result<RetryNumber, RetryNumberError> {
        u32::try_from(ordinal)
            .ok()
            .and_then(NonZeroU32::new)
            .map(RetryNumber)
            .ok_or(RetryNumberError(i64::from(ordinal)))
    }
}

#[cfg(test)]
mod tests {
    use crate::retry_number::{RetryNumber, RetryNumberError};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    #[test]
    fn zero_names_no_retry() {
        assert_eq!(RetryNumber::try_from(0_u32), Err(RetryNumberError(0)));
    }

    #[test]
    fn a_negative_count_names_no_retry() {
        assert_eq!(RetryNumber::try_from(-1_i32), Err(RetryNumberError(-1)));
    }

    #[test]
    fn a_positive_count_names_the_retry_at_that_ordinal() {
        assert_eq!(RetryNumber::try_from(3_i32).map(RetryNumber::get), Ok(3));
        assert_eq!(RetryNumber::try_from(1_u32), Ok(RetryNumber::FIRST));
    }

    #[test]
    fn the_last_retry_is_its_own_successor() {
        let last = RetryNumber::new(NonZeroU32::MAX);
        assert_eq!(last.next(), last);
    }

    #[test]
    fn the_successor_is_one_ordinal_later() {
        assert_eq!(RetryNumber::FIRST.next().get(), 2);
    }
}
