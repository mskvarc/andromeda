//! What a store failed with, classified independently of the backend.
//!
//! The classification is closed and carries no text: an adapter maps its own
//! error onto one variant and records that error for operators with
//! [`PersistenceFailure::record`], so the backend's detail reaches the log and
//! never the value a port hands to policy, and from there to a client.

use derive_more::IsVariant;
use std::{fmt::Display, panic::Location};
use thiserror::Error;

/// The class of failure a persistence adapter met, independent of its backend.
///
/// Two variants are backpressure rather than faults. [`Saturated`] and
/// [`RolledBack`] describe load and contention: the operation did not persist
/// and can run again unchanged, so each port routes them to its own retryable
/// category instead of its fault category. Every other variant is a fault the
/// client cannot act on.
///
/// [`Saturated`]: PersistenceFailure::Saturated
/// [`RolledBack`]: PersistenceFailure::RolledBack
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error, IsVariant)]
pub enum PersistenceFailure {
    /// No connection to the store became free within the acquire deadline, or
    /// the store is not accepting work right now.
    #[error("the store is saturated")]
    Saturated,
    /// The store rolled the transaction back to break a deadlock or a
    /// serialization conflict with a concurrent transaction.
    #[error("the store rolled the transaction back under contention")]
    RolledBack,
    /// The store could not be reached, or the connection to it failed.
    #[error("the store is unreachable")]
    Unavailable,
    /// The store refused a write that would violate one of its integrity
    /// constraints.
    #[error("the store refused a write that violates an integrity constraint")]
    ConstraintViolated,
    /// The store refused or failed a statement for a reason other than an
    /// integrity constraint.
    #[error("the store rejected a statement")]
    StatementRejected,
    /// A stored record the operation depends on is absent.
    #[error("a stored record the operation depends on is missing")]
    RecordMissing,
    /// A stored value no longer reads as the domain value it was written from.
    #[error("a stored value could not be decoded")]
    Undecodable,
    /// A domain value could not be encoded into the store's representation.
    #[error("a value could not be encoded for storage")]
    Unencodable,
    /// The adapter could not build the statement it needed.
    #[error("a store statement could not be built")]
    StatementUnbuildable,
    /// The store refused to grow past a bound it is configured with.
    #[error("the store refused to grow past its configured bound")]
    CapacityExceeded,
    /// The store's schema is not the one this broker was built against.
    #[error("the store schema does not match this broker")]
    SchemaMismatch,
    /// The store answered in a way the adapter's contract with it rules out.
    #[error("the store broke the adapter's contract")]
    ContractBroken,
}

impl PersistenceFailure {
    /// Record the adapter error this failure classifies, then hand the failure on.
    ///
    /// The cause is logged with the caller's source location so an operator can
    /// find the classifying site; only the classification travels onward.
    /// Backpressure is expected under load and is logged at debug level, every
    /// fault at error level.
    #[track_caller]
    #[must_use]
    pub fn record(self, cause: &dyn Display) -> PersistenceFailure {
        let location = Location::caller();
        if self.is_backpressure() {
            tracing::debug!(failure = %self, %cause, %location, "persistence adapter backpressure");
        } else {
            tracing::error!(failure = %self, %cause, %location, "persistence adapter failure");
        }
        self
    }

    /// Record a failure that has no adapter error of its own, such as a row the
    /// store should have returned and did not.
    #[track_caller]
    #[must_use]
    pub fn record_bare(self) -> PersistenceFailure {
        let location = Location::caller();
        tracing::error!(failure = %self, %location, "persistence adapter failure");
        self
    }

    /// Whether this failure is load or contention rather than a fault: the
    /// operation did not persist and can run again unchanged.
    #[must_use]
    pub const fn is_backpressure(self) -> bool {
        matches!(self, PersistenceFailure::Saturated | PersistenceFailure::RolledBack)
    }
}

#[cfg(test)]
mod tests {
    use crate::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;

    #[test]
    fn saturation_and_rollback_are_backpressure() {
        assert!(PersistenceFailure::Saturated.is_backpressure());
        assert!(PersistenceFailure::RolledBack.is_backpressure());
    }

    #[test]
    fn every_other_failure_is_a_fault() {
        for failure in [
            PersistenceFailure::Unavailable,
            PersistenceFailure::ConstraintViolated,
            PersistenceFailure::StatementRejected,
            PersistenceFailure::RecordMissing,
            PersistenceFailure::Undecodable,
            PersistenceFailure::Unencodable,
            PersistenceFailure::StatementUnbuildable,
            PersistenceFailure::CapacityExceeded,
            PersistenceFailure::SchemaMismatch,
            PersistenceFailure::ContractBroken,
        ] {
            assert!(!failure.is_backpressure(), "{failure}");
        }
    }

    #[test]
    fn recording_hands_the_same_failure_on() {
        assert_eq!(PersistenceFailure::Undecodable.record(&"cause"), PersistenceFailure::Undecodable);
        assert_eq!(PersistenceFailure::RecordMissing.record_bare(), PersistenceFailure::RecordMissing);
    }

    #[test]
    fn the_rendering_names_the_class_and_no_backend_detail() {
        assert_eq!(PersistenceFailure::Undecodable.to_string(), "a stored value could not be decoded");
        assert_eq!(PersistenceFailure::Saturated.to_string(), "the store is saturated");
    }
}
