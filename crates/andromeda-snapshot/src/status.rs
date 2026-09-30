//! Snapshot state-machine transitions.
//!
//! Pure business logic: no I/O, no SQL. Called by the snapshot executor
//! before any `update_status` request reaches the repository.

use crate::{branch_condition::BranchCondition, error::SnapshotError, snapshot_validation_reason::SnapshotValidationReason};
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
    /// The stored branch is behind this binary and cannot be read at all.
    Quarantined,
}

/// Decide whether a snapshot in `status` whose stored branch is in `condition`
/// can scope a request.
///
/// A quarantined branch is refused whatever the status: a preparing Snapshot
/// whose branch is behind would never be prepared, and waiting for it would
/// only delay the same refusal.
#[must_use]
pub const fn scoping_eligibility(status: SnapshotStatus, condition: BranchCondition) -> ScopingEligibility {
    match (condition, status) {
        (BranchCondition::Quarantined, _) => ScopingEligibility::Quarantined,
        (BranchCondition::Current, SnapshotStatus::Success | SnapshotStatus::Partial | SnapshotStatus::Empty | SnapshotStatus::Failure) => {
            ScopingEligibility::Usable
        }
        (BranchCondition::Current, SnapshotStatus::Preparing) => ScopingEligibility::Preparing,
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
        Err(SnapshotValidationReason::StatusTransition { status }.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        branch_condition::BranchCondition,
        status::{ScopingEligibility, scoping_eligibility, validate_preparing_transition},
    };
    use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn empty_partial_and_success_snapshots_can_scope_a_request() {
        for status in [SnapshotStatus::Empty, SnapshotStatus::Partial, SnapshotStatus::Success] {
            assert_eq!(scoping_eligibility(status, BranchCondition::Current), ScopingEligibility::Usable, "{status}");
        }
    }

    #[test]
    fn terminal_failure_remains_addressable_and_preparing_waits() {
        assert_eq!(
            scoping_eligibility(SnapshotStatus::Failure, BranchCondition::Current),
            ScopingEligibility::Usable
        );
        assert_eq!(
            scoping_eligibility(SnapshotStatus::Preparing, BranchCondition::Current),
            ScopingEligibility::Preparing
        );
    }

    #[test]
    fn a_quarantined_branch_is_refused_whatever_the_status() {
        for status in SnapshotStatus::iter() {
            assert_eq!(
                scoping_eligibility(status, BranchCondition::Quarantined),
                ScopingEligibility::Quarantined,
                "{status}"
            );
        }
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
