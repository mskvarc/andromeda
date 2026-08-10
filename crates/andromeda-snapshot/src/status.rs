//! Snapshot state-machine transitions.
//!
//! Pure business logic: no I/O, no SQL. Called by the snapshot executor
//! before any `update_status` request reaches the repository.

use crate::error::SnapshotError;
use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;

/// Validate that a `preparing -> status` transition is permitted by the
/// snapshot state machine (ETSI GS CIM 009 clause 5.16.4).
pub fn validate_preparing_transition(status: SnapshotStatus) -> Result<(), SnapshotError> {
    if SnapshotStatus::Preparing.can_transition_to(status) {
        Ok(())
    } else {
        Err(SnapshotError::validation(format!("Invalid status transition: preparing -> {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::validate_preparing_transition;
    use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;

    #[test]
    fn preparing_to_success_is_valid() {
        assert!(validate_preparing_transition(SnapshotStatus::Success).is_ok());
    }

    #[test]
    fn preparing_to_preparing_rejected() {
        assert!(validate_preparing_transition(SnapshotStatus::Preparing).is_err());
    }
}
