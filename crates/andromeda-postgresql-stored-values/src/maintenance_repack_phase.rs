//! The persisted phase of one running table repack.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The phase a running repack of one table has reached.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case. A run
    /// that stops past `setup` leaves objects in the tenant database, which
    /// recovery removes; the phase says how far the run got.
    MaintenanceRepackPhaseCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 6,
        frozen_digest: 0x0ff7_f970_9cc4_c834,
        codes: {
            /// The log table and the trigger filling it are being created.
            Setup = 1, "setup";
            /// The table's rows are being copied.
            Copy = 2, "copy";
            /// The copy's indexes are being built.
            IndexBuild = 3, "index-build";
            /// The writes logged since the copy are being replayed.
            Replay = 4, "replay";
            /// The table is being given the copy's files.
            Swap = 5, "swap";
            /// The run's objects, the old files among them, are being dropped.
            Drop = 6, "drop";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_repack_phase::MaintenanceRepackPhaseCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceRepackPhaseCode::ALL.len(), 6);
        assert_eq!(MaintenanceRepackPhaseCode::Setup.code(), 1);
        assert_eq!(MaintenanceRepackPhaseCode::Drop.code(), 6);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceRepackPhaseCode::try_from(0).is_err());
        assert!(MaintenanceRepackPhaseCode::try_from(7).is_err());
    }
}
