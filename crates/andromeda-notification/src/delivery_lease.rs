//! The lease a worker holds over a claimed notification delivery.
//!
//! A claim without an expiry is not a lease: once the claiming transaction
//! commits and releases its row lock, nothing stops a second worker from
//! claiming a still-due delivery and sending the same notification again. The
//! lease term bounds that ownership, and
//! [`LeaseAcknowledgement`](andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement)
//! records what happened when a worker tried to act on a delivery whose lease
//! it may no longer hold.

use crate::delivery_entry::DeliveryKey;
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use chrono::{DateTime, Duration, Utc};
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

    /// The term as a signed duration.
    #[must_use]
    pub fn as_duration(self) -> Duration {
        Duration::seconds(i64::from(self.seconds()))
    }

    /// The instant at which a lease taken at `now` expires.
    ///
    /// The authoritative deadline is computed by the database from its own
    /// clock; this projection exists so a worker can report the deadline it
    /// expects. `None` means the addition would leave the representable
    /// timestamp range, which no reachable claim clock produces.
    #[must_use]
    pub fn expires_at(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        now.checked_add_signed(self.as_duration())
    }
}

/// Failure while acting on a leased notification delivery.
#[derive(Debug, Error)]
pub enum DeliveryLeaseError {
    /// The delivery is no longer leased by this worker: its lease expired and
    /// another worker reclaimed it, or the delivery was removed. Acknowledging
    /// it would overwrite state this worker no longer owns.
    #[error("notification delivery {delivery} is no longer leased by {worker}")]
    LeaseLost {
        /// The delivery the worker tried to acknowledge.
        delivery: DeliveryKey,
        /// The worker whose lease was not honoured.
        worker: LeaseHolder,
    },
    /// The acknowledging statement itself failed.
    #[error("notification delivery lease acknowledgement failed: {0}")]
    Persistence(#[source] PersistenceFailure),
}

impl DeliveryLeaseError {
    /// Construct the lost-lease failure for one delivery and worker.
    #[must_use]
    pub const fn lease_lost(delivery: DeliveryKey, worker: &LeaseHolder) -> DeliveryLeaseError {
        DeliveryLeaseError::LeaseLost { delivery, worker: *worker }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_entry::DeliveryKey,
        delivery_lease::{DeliveryLeaseError, LeaseTerm, LeaseTermError},
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

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
    fn lease_term_reports_its_seconds_and_duration() -> Result<(), LeaseTermError> {
        let term = LeaseTerm::from_seconds(30)?;
        assert_eq!(term.seconds(), 30);
        assert_eq!(term.as_duration().num_seconds(), 30);
        Ok(())
    }

    #[test]
    fn lease_expiry_is_the_claim_instant_plus_the_term() -> Result<(), LeaseTermError> {
        let term = LeaseTerm::from_seconds(45)?;
        let now = DateTime::<Utc>::UNIX_EPOCH;
        assert_eq!(term.expires_at(now), Some(now + term.as_duration()));
        assert_eq!(term.expires_at(now).map(|at| at.timestamp()), Some(45));
        Ok(())
    }

    #[test]
    fn lost_lease_failure_names_the_delivery_and_the_worker() {
        let worker = LeaseHolder::from(Uuid::nil());
        let error = DeliveryLeaseError::lease_lost(DeliveryKey::from(7_i64), &worker);
        assert_eq!(
            error.to_string(),
            "notification delivery 7 is no longer leased by 00000000-0000-0000-0000-000000000000"
        );
    }
}
