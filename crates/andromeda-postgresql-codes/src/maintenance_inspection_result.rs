//! The persisted outcome of one tenant's maintenance inspection.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// What the last maintenance inspection of a tenant came to.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case.
    MaintenanceInspectionResultCode {
        frozen_prefix: 20,
        frozen_digest: 0xaeac_39e8_0ec4_152b,
        codes: {
            /// The inspection started and has not finished.
            Started = 1, "started";
            /// The tenant database could not be reached.
            Unavailable = 2, "unavailable";
            /// Every capability the controller needs is present.
            Ready = 3, "ready";
            /// The `pgstattuple` extension is absent.
            MissingPgstattuple = 4, "missing-pgstattuple";
            /// The broker role may not execute `pgstattuple`.
            MissingPgstattupleExecute = 5, "missing-pgstattuple-execute";
            /// A visit stopped at a missing `MAINTAIN` privilege. Nothing
            /// writes it: such a visit observes and records its own outcome.
            /// It decodes so a stored row keeps its meaning.
            MissingMaintain = 6, "missing-maintain";
            /// The controller is disabled.
            Disabled = 7, "disabled";
            /// Another replica holds the fleet-wide lock.
            GlobalLockUnavailable = 8, "global-lock-unavailable";
            /// No tenant was due.
            NoTenant = 9, "no-tenant";
            /// The selected tenant was unavailable.
            TenantUnavailable = 10, "tenant-unavailable";
            /// The tenant schema is behind the binary.
            SchemaBehind = 11, "schema-behind";
            /// A required capability is missing.
            CapabilityUnavailable = 12, "capability-unavailable";
            /// Nothing needed attention.
            Idle = 13, "idle";
            /// A detailed probe ran.
            Probed = 14, "probed";
            /// An action was queued.
            Queued = 15, "queued";
            /// An action is due but acting is disabled.
            WouldAct = 16, "would-act";
            /// An action was deferred.
            Deferred = 17, "deferred";
            /// An action ran.
            Acted = 18, "acted";
            /// An action failed.
            ActionFailed = 19, "action-failed";
            /// The pass was cancelled.
            Cancelled = 20, "cancelled";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_inspection_result::MaintenanceInspectionResultCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceInspectionResultCode::ALL.len(), 20);
        assert_eq!(MaintenanceInspectionResultCode::Started.code(), 1);
        assert_eq!(MaintenanceInspectionResultCode::Cancelled.code(), 20);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceInspectionResultCode::try_from(0).is_err());
        assert!(MaintenanceInspectionResultCode::try_from(21).is_err());
    }
}
