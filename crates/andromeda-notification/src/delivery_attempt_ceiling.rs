//! The number of delivery attempts after which a notification is dead-lettered.
//!
//! The worker, not the schema, owns the ceiling: the attempt that reaches it is
//! acknowledged as exhausted, which stamps the delivery's `exhausted_at` and
//! takes it out of the claimable set for good. The queue never repeats the
//! number, neither in a check constraint nor in the claim index, so the
//! setting can be raised or lowered without a migration. The only bound
//! validated at configuration time is that the store can count that high.

use andromeda_retry::retry_number::RetryNumber;
use std::num::{NonZeroI32, NonZeroU32};
use thiserror::Error;

/// Rejected delivery-attempt ceiling.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryAttemptCeilingError {
    /// The store counts a delivery's attempts in a signed 32-bit column, so a
    /// ceiling past it could never be reached.
    #[error("notification delivery attempt ceiling of {attempts} exceeds the {maximum} attempts the store counts")]
    AboveStoredRange {
        /// The rejected ceiling.
        attempts: u32,
        /// The most attempts the store counts.
        maximum: i32,
    },
}

/// The number of attempts a delivery is given before it is dead-lettered.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeliveryAttemptCeiling(NonZeroI32);

impl DeliveryAttemptCeiling {
    /// Build a ceiling from a whole number of attempts.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryAttemptCeilingError::AboveStoredRange`] when the
    /// number exceeds what the store counts.
    pub fn from_attempts(attempts: NonZeroU32) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
        NonZeroI32::try_from(attempts)
            .map(DeliveryAttemptCeiling)
            .map_err(|_| DeliveryAttemptCeilingError::AboveStoredRange {
                attempts: attempts.get(),
                maximum: i32::MAX,
            })
    }

    /// The ceiling as a whole number of attempts.
    #[must_use]
    pub const fn attempts(self) -> i32 {
        self.0.get()
    }

    /// The retry that follows the last attempt the ceiling allows, where a
    /// delivery's backoff stops growing.
    #[must_use]
    pub const fn last_retry(self) -> RetryNumber {
        RetryNumber::new(NonZeroU32::MIN.saturating_add(self.0.get().unsigned_abs()))
    }

    /// Whether an attempt made after `attempts_before` earlier attempts is the
    /// last one the ceiling allows.
    ///
    /// A delivery claimed with `attempts_before` failed attempts on record is
    /// about to make attempt `attempts_before + 1`; when that number reaches
    /// the ceiling, a failure of this attempt exhausts the delivery instead of
    /// scheduling another retry.
    #[must_use]
    pub const fn is_final_attempt(self, attempts_before: i32) -> bool {
        attempts_before.saturating_add(1) >= self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn attempts(count: u32) -> NonZeroU32 {
        NonZeroU32::new(count).expect("a positive count")
    }

    #[test]
    fn a_ceiling_past_the_stored_range_is_rejected_and_the_range_end_accepted() {
        assert_eq!(
            DeliveryAttemptCeiling::from_attempts(attempts(2_147_483_648)),
            Err(DeliveryAttemptCeilingError::AboveStoredRange {
                attempts: 2_147_483_648,
                maximum: i32::MAX,
            })
        );
        assert!(DeliveryAttemptCeiling::from_attempts(attempts(2_147_483_647)).is_ok());
    }

    #[test]
    fn a_ceiling_above_five_is_accepted_because_the_schema_no_longer_pins_one() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(attempts(12)).expect("any positive ceiling is accepted");
        assert_eq!(ceiling.attempts(), 12);
    }

    #[test]
    fn the_attempt_that_reaches_the_ceiling_is_the_final_one() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(attempts(3)).expect("a positive ceiling");
        assert!(!ceiling.is_final_attempt(0));
        assert!(!ceiling.is_final_attempt(1));
        assert!(ceiling.is_final_attempt(2));
        assert!(ceiling.is_final_attempt(7));
    }

    #[test]
    fn a_ceiling_of_one_makes_the_first_attempt_final() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(attempts(1)).expect("a positive ceiling");
        assert!(ceiling.is_final_attempt(0));
    }
}
