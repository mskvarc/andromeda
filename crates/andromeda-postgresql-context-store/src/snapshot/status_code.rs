//! The number the `snapshot_preparation.status` column stores a status as.
//!
//! Written as exhaustive matches in both directions, so a status a later
//! revision of clause 5.2.22 adds fails to compile here, where its persisted
//! number is decided.

use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;

/// The registry member the store persists for a Snapshot status.
#[must_use]
pub const fn stored_status(status: SnapshotStatus) -> SnapshotStatusCode {
    match status {
        SnapshotStatus::Preparing => SnapshotStatusCode::Preparing,
        SnapshotStatus::Success => SnapshotStatusCode::Success,
        SnapshotStatus::Partial => SnapshotStatusCode::Partial,
        SnapshotStatus::Empty => SnapshotStatusCode::Empty,
        SnapshotStatus::Failure => SnapshotStatusCode::Failure,
    }
}

/// The Snapshot status a stored registry member names.
#[must_use]
pub const fn status_of(code: SnapshotStatusCode) -> SnapshotStatus {
    match code {
        SnapshotStatusCode::Preparing => SnapshotStatus::Preparing,
        SnapshotStatusCode::Success => SnapshotStatus::Success,
        SnapshotStatusCode::Partial => SnapshotStatus::Partial,
        SnapshotStatusCode::Empty => SnapshotStatus::Empty,
        SnapshotStatusCode::Failure => SnapshotStatus::Failure,
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot::status_code::{status_of, stored_status};
    use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;
    use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_snapshot_status_survives_a_round_trip_through_its_stored_number() {
        for status in SnapshotStatus::iter() {
            assert_eq!(status_of(stored_status(status)), status);
        }
    }

    /// The registry's label is the clause 5.2.22 token the domain enum renders.
    #[test]
    fn the_stored_number_of_a_status_is_the_one_its_token_is_registered_under() {
        for code in SnapshotStatusCode::ALL {
            assert_eq!(code.label(), status_of(*code).to_string());
        }
    }
}
