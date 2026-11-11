//! The lease a worker holds over a claimed notification delivery.
//!
//! A claim without an expiry is not a lease: once the claiming transaction
//! commits and releases its row lock, nothing stops a second worker from
//! claiming a still-due delivery and sending the same notification again. The
//! lease term bounds that ownership, and
//! [`LeaseAcknowledgement`](andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement)
//! records what happened when a worker tried to act on a delivery whose lease
//! it may no longer hold.

use derive_more::Display;
use std::num::NonZeroU32;
use thiserror::Error;

/// Rejected notification delivery lease term.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LeaseTermError {
    /// A zero-second lease expires the instant it is taken, so every claim is
    /// immediately reclaimable and the lease provides no exclusion at all.
    #[error("notification delivery lease term must be at least one second")]
    ZeroSeconds,
    /// A lease longer than a day strands a crashed worker's deliveries for
    /// longer than any delivery attempt can justify.
    #[error("notification delivery lease term of {seconds} seconds exceeds the maximum of {maximum} seconds")]
    TooLong {
        /// The rejected term.
        seconds: u32,
        /// The largest accepted term.
        maximum: u32,
    },
}

/// How long a worker owns a claimed delivery before another worker may reclaim it.
#[derive(Clone, Copy, Debug, Display, Eq, Ord, PartialEq, PartialOrd)]
#[display("{_0}s")]
pub struct LeaseTerm(NonZeroU32);

impl LeaseTerm {
    /// The longest accepted lease term, in seconds.
    pub const MAX_SECONDS: u32 = 86_400;

    /// Build a lease term from a whole number of seconds.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseTermError::ZeroSeconds`] when `seconds` is zero, and
    /// [`LeaseTermError::TooLong`] when it exceeds [`LeaseTerm::MAX_SECONDS`].
    pub const fn from_seconds(seconds: u32) -> Result<LeaseTerm, LeaseTermError> {
        if seconds > LeaseTerm::MAX_SECONDS {
            return Err(LeaseTermError::TooLong {
                seconds,
                maximum: LeaseTerm::MAX_SECONDS,
            });
        }
        match NonZeroU32::new(seconds) {
            Some(seconds) => Ok(LeaseTerm(seconds)),
            None => Err(LeaseTermError::ZeroSeconds),
        }
    }

    /// The term as a whole number of seconds.
    #[must_use]
    pub const fn seconds(self) -> u32 {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_lease::{LeaseTerm, LeaseTermError};
    use pretty_assertions::assert_eq;

    #[test]
    fn zero_second_lease_term_is_rejected() {
        assert_eq!(LeaseTerm::from_seconds(0), Err(LeaseTermError::ZeroSeconds));
    }

    #[test]
    fn lease_term_beyond_one_day_is_rejected() {
        assert_eq!(
            LeaseTerm::from_seconds(86_401),
            Err(LeaseTermError::TooLong {
                seconds: 86_401,
                maximum: 86_400,
            })
        );
    }

    #[test]
    fn lease_term_reports_its_seconds_and_reads_as_them() -> Result<(), LeaseTermError> {
        let term = LeaseTerm::from_seconds(30)?;
        assert_eq!(term.seconds(), 30);
        assert_eq!(term.to_string(), "30s");
        Ok(())
    }
}
