//! The persisted lifecycle state of one snapshot.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The lifecycle state of a Snapshot (clause 5.2.22 `status`).
    ///
    /// The labels are the specification's own status tokens.
    SnapshotStatusCode {
        frozen_prefix: 5,
        frozen_digest: 0x00b7_ddb9_0187_109f,
        codes: {
            /// The snapshot is being prepared.
            Preparing = 1, "preparing";
            /// Every query was captured.
            Success = 2, "success";
            /// Some queries were captured.
            Partial = 3, "partial";
            /// The queries matched nothing.
            Empty = 4, "empty";
            /// The preparation failed.
            Failure = 5, "failure";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot_status::SnapshotStatusCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(SnapshotStatusCode::ALL.len(), 5);
        assert_eq!(SnapshotStatusCode::Preparing.code(), 1);
        assert_eq!(SnapshotStatusCode::Failure.code(), 5);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(SnapshotStatusCode::try_from(0).is_err());
        assert!(SnapshotStatusCode::try_from(6).is_err());
    }
}
