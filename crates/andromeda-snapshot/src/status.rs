//! Snapshot state-machine transitions.
//!
//! Pure business logic: no I/O, no SQL. Called by the snapshot executor
//! before any `update_status` request reaches the repository.

use crate::error::SnapshotError;
use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;

/// Whether a snapshot in a given status can scope a request (clause 6.3.18).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopingEligibility {
    /// The snapshot's frozen dataset is complete and may be read, even when
    /// it is empty (clause 5.2.40: `empty` is a finished preparation whose
    /// queries matched nothing).
    Usable,
    /// Capture or clone promotion has not yet committed.
    Preparing,
}

/// Decide whether a snapshot in `status` can scope a request.
#[must_use]
pub const fn scoping_eligibility(status: SnapshotStatus) -> ScopingEligibility {
    match status {
        SnapshotStatus::Success | SnapshotStatus::Partial | SnapshotStatus::Empty | SnapshotStatus::Failure => ScopingEligibility::Usable,
        SnapshotStatus::Preparing => ScopingEligibility::Preparing,
    }
}

/// Validate that a `preparing -> status` transition is permitted by the
/// snapshot state machine (ETSI GS CIM 009 clause 5.16.4).
///
/// # Errors
///
/// Returns [`SnapshotError::Validation`] when the state machine forbids
/// `preparing -> status`, which is the case for `preparing` itself.
pub fn validate_preparing_transition(status: SnapshotStatus) -> Result<(), SnapshotError> {
    if SnapshotStatus::Preparing.can_transition_to(status) {
        Ok(())
    } else {
        Err(SnapshotError::validation(format!("Invalid status transition: preparing -> {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::{ScopingEligibility, scoping_eligibility, validate_preparing_transition};
    use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_partial_and_success_snapshots_can_scope_a_request() {
        for status in [SnapshotStatus::Empty, SnapshotStatus::Partial, SnapshotStatus::Success] {
            assert_eq!(scoping_eligibility(status), ScopingEligibility::Usable, "{status}");
        }
    }

    #[test]
    fn terminal_failure_remains_addressable_and_preparing_waits() {
        assert_eq!(scoping_eligibility(SnapshotStatus::Failure), ScopingEligibility::Usable);
        assert_eq!(scoping_eligibility(SnapshotStatus::Preparing), ScopingEligibility::Preparing);
    }

    #[test]
    fn preparing_to_success_is_valid() {
        assert!(validate_preparing_transition(SnapshotStatus::Success).is_ok());
    }

    #[test]
    fn preparing_to_preparing_rejected() {
        assert!(validate_preparing_transition(SnapshotStatus::Preparing).is_err());
    }
}
