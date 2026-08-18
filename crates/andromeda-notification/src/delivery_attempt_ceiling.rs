//! The number of delivery attempts after which a notification is dead-lettered.
//!
//! The worker, not the schema, owns the ceiling: the attempt that reaches it is
//! acknowledged as exhausted, which stamps the delivery's `exhausted_at` and
//! takes it out of the claimable set for good. The queue never repeats the
//! number — neither in a check constraint nor in the claim index — so the
//! setting can be raised or lowered without a migration. The only bound
//! validated at configuration time is that at least one attempt is allowed.

use std::num::NonZeroI32;
use thiserror::Error;

/// Rejected delivery-attempt ceiling.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryAttemptCeilingError {
    /// A ceiling of zero or less leaves every delivery permanently exhausted.
    #[error("notification delivery attempt ceiling must be at least one attempt")]
    NonPositiveAttempts,
}

/// The number of attempts a delivery is given before it is dead-lettered.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeliveryAttemptCeiling(NonZeroI32);

impl DeliveryAttemptCeiling {
    /// Build a ceiling from a whole number of attempts.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryAttemptCeilingError::NonPositiveAttempts`] when the
    /// number is zero or negative.
    pub const fn from_attempts(attempts: i32) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
        match NonZeroI32::new(attempts) {
            Some(attempts) if attempts.get() > 0 => Ok(DeliveryAttemptCeiling(attempts)),
            Some(_) | None => Err(DeliveryAttemptCeilingError::NonPositiveAttempts),
        }
    }

    /// The ceiling as a whole number of attempts.
    pub const fn attempts(self) -> i32 {
        self.0.get()
    }

    /// Whether an attempt made after `attempts_before` earlier attempts is the
    /// last one the ceiling allows.
    ///
    /// A delivery claimed with `attempts_before` failed attempts on record is
    /// about to make attempt `attempts_before + 1`; when that number reaches
    /// the ceiling, a failure of this attempt exhausts the delivery instead of
    /// scheduling another retry.
    pub const fn is_final_attempt(self, attempts_before: i32) -> bool {
        attempts_before.saturating_add(1) >= self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_attempt_ceiling::{DeliveryAttemptCeiling, DeliveryAttemptCeilingError};

    #[test]
    fn a_non_positive_ceiling_is_rejected() {
        for attempts in [0, -1] {
            assert_eq!(
                DeliveryAttemptCeiling::from_attempts(attempts),
                Err(DeliveryAttemptCeilingError::NonPositiveAttempts)
            );
        }
    }

    #[test]
    fn a_ceiling_above_five_is_accepted_because_the_schema_no_longer_pins_one() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(12).expect("any positive ceiling is accepted");
        assert_eq!(ceiling.attempts(), 12);
    }

    #[test]
    fn the_attempt_that_reaches_the_ceiling_is_the_final_one() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(3).expect("a positive ceiling");
        assert!(!ceiling.is_final_attempt(0));
        assert!(!ceiling.is_final_attempt(1));
        assert!(ceiling.is_final_attempt(2));
        assert!(ceiling.is_final_attempt(7));
    }

    #[test]
    fn a_ceiling_of_one_makes_the_first_attempt_final() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(1).expect("a positive ceiling");
        assert!(ceiling.is_final_attempt(0));
    }
}
