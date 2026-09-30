//! The persisted outcome of one maintenance action.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// How the last maintenance action on a target ended.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case.
    MaintenanceActionResultCode {
        frozen_prefix: 2,
        frozen_digest: 0x144f_c6ff_550b_9522,
        codes: {
            /// The action succeeded.
            Succeeded = 1, "succeeded";
            /// The action failed.
            Failed = 2, "failed";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_action_result::MaintenanceActionResultCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceActionResultCode::ALL.len(), 2);
        assert_eq!(MaintenanceActionResultCode::Succeeded.code(), 1);
        assert_eq!(MaintenanceActionResultCode::Failed.code(), 2);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceActionResultCode::try_from(0).is_err());
        assert!(MaintenanceActionResultCode::try_from(3).is_err());
    }
}
