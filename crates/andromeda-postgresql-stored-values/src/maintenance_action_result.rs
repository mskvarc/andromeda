//! The persisted outcome of one maintenance action.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// How the last maintenance action on a target ended.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case.
    MaintenanceActionResultCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 7,
        frozen_digest: 0xb861_040e_5e11_ec46,
        codes: {
            /// The action succeeded.
            Succeeded = 1, "succeeded";
            /// The action failed.
            Failed = 2, "failed";
            /// The action completed without the effect it was queued for.
            Ineffective = 3, "ineffective";
            /// A repack was abandoned: its `ACCESS EXCLUSIVE` lock was not
            /// granted within the wait budget, and no other session was
            /// cancelled to get it.
            RepackAbandonedLockWait = 4, "repack-abandoned-lock-wait";
            /// A repack was abandoned: writes kept its log from shrinking to
            /// the point where it could swap.
            RepackNotConverging = 5, "repack-not-converging";
            /// A repack was abandoned to let another session take a lock on
            /// the table it was copying.
            RepackYielded = 6, "repack-yielded";
            /// The objects an interrupted repack left were cleaned up.
            RepackRecovered = 7, "repack-recovered";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_action_result::MaintenanceActionResultCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceActionResultCode::ALL.len(), 7);
        assert_eq!(MaintenanceActionResultCode::Succeeded.code(), 1);
        assert_eq!(MaintenanceActionResultCode::Failed.code(), 2);
        assert_eq!(MaintenanceActionResultCode::Ineffective.code(), 3);
        assert_eq!(MaintenanceActionResultCode::RepackAbandonedLockWait.code(), 4);
        assert_eq!(MaintenanceActionResultCode::RepackRecovered.code(), 7);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceActionResultCode::try_from(0).is_err());
        assert!(MaintenanceActionResultCode::try_from(8).is_err());
    }
}
