//! The number of delivery attempts after which a notification is dead-lettered.
//!
//! The schema, not the worker, owns the ceiling: the migration that creates the
//! delivery queue writes the number into `chk_notification_deliveries_exhausted_state`
//! and into the claim's partial index, so a worker configured to attempt more
//! would claim a delivery the check constraint then refuses — aborting the claim
//! and stalling the queue rather than merely losing an index. The bound is
//! therefore validated at configuration time, before any delivery is claimed.

use std::num::NonZeroI32;
use thiserror::Error;

/// Rejected delivery-attempt ceiling.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryAttemptCeilingError {
    /// A ceiling of zero or less leaves every delivery permanently exhausted.
    #[error("notification delivery attempt ceiling must be at least one attempt")]
    NonPositiveAttempts,
    /// A ceiling above what the schema constrains would make the claim abort on
    /// a check violation instead of dead-lettering the delivery.
    #[error("notification delivery attempt ceiling of {attempts} exceeds the schema maximum of {maximum}")]
    AboveSchemaMaximum {
        /// The rejected ceiling.
        attempts: i32,
        /// The largest ceiling the schema accepts.
        maximum: i32,
    },
}

/// The number of attempts a delivery is given before it is dead-lettered.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeliveryAttemptCeiling(NonZeroI32);

impl DeliveryAttemptCeiling {
    /// The largest ceiling the delivery schema accepts.
    pub const SCHEMA_MAXIMUM_ATTEMPTS: i32 = 5;

    /// Build a ceiling from a whole number of attempts.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryAttemptCeilingError`] when the number is not positive
    /// or exceeds what the delivery schema constrains.
    pub const fn from_attempts(attempts: i32) -> Result<DeliveryAttemptCeiling, DeliveryAttemptCeilingError> {
        if attempts > DeliveryAttemptCeiling::SCHEMA_MAXIMUM_ATTEMPTS {
            return Err(DeliveryAttemptCeilingError::AboveSchemaMaximum {
                attempts,
                maximum: DeliveryAttemptCeiling::SCHEMA_MAXIMUM_ATTEMPTS,
            });
        }
        match NonZeroI32::new(attempts) {
            Some(attempts) if attempts.get() > 0 => Ok(DeliveryAttemptCeiling(attempts)),
            Some(_) | None => Err(DeliveryAttemptCeilingError::NonPositiveAttempts),
        }
    }

    /// The ceiling as a whole number of attempts.
    pub const fn attempts(self) -> i32 {
        self.0.get()
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

    /// The check constraint the delivery table carries names the literal 5, so
    /// a claim above it aborts the transaction rather than dead-lettering.
    #[test]
    fn a_ceiling_above_the_schema_maximum_is_rejected() {
        assert_eq!(
            DeliveryAttemptCeiling::from_attempts(6),
            Err(DeliveryAttemptCeilingError::AboveSchemaMaximum { attempts: 6, maximum: 5 })
        );
    }

    #[test]
    fn a_ceiling_reports_the_attempt_count_it_was_built_from() {
        let ceiling = DeliveryAttemptCeiling::from_attempts(5).expect("the schema maximum is accepted");
        assert_eq!(ceiling.attempts(), 5);
    }
}
