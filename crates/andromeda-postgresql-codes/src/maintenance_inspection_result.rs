//! The persisted outcome of one tenant's maintenance inspection.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// What the last maintenance inspection of a tenant came to.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case.
    MaintenanceInspectionResultCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 23,
        frozen_digest: 0xc0d3_1156_f215_bc14,
        codes: {
            /// The inspection started and has not finished.
            Started = 1, "started";
            /// The tenant database could not be reached.
            Unavailable = 2, "unavailable";
            /// A capability check found every capability present. Nothing
            /// writes it: a visit records its own outcome. It decodes so a
            /// stored row keeps its meaning.
            Ready = 3, "ready";
            /// A visit stopped at an absent `pgstattuple` extension. Nothing
            /// writes it: such a visit probes no index and records its own
            /// outcome. It decodes so a stored row keeps its meaning.
            MissingPgstattuple = 4, "missing-pgstattuple";
            /// A visit stopped at a role that may not execute `pgstattuple`.
            /// Nothing writes it: such a visit probes no index and records
            /// its own outcome. It decodes so a stored row keeps its meaning.
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
            /// A visit stopped at a missing capability. Nothing writes it: a
            /// missing capability narrows a visit and never ends it. It
            /// decodes so a stored row keeps its meaning.
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
            /// A visit found no `pg_repack` extension. Nothing writes it: such a
            /// visit repacks no table and records its own outcome. It names
            /// the missing capability in the log.
            MissingPgRepack = 21, "missing-pg-repack";
            /// A visit found a `pg_repack` version the controller does not
            /// drive. Nothing writes it: such a visit repacks no table and
            /// records its own outcome. It names the missing capability in
            /// the log.
            UnsupportedPgRepack = 22, "unsupported-pg-repack";
            /// A visit found a role that may not run a repack. Nothing writes
            /// it: such a visit only observes its repacks and records its own
            /// outcome. It names the missing capability in the log.
            MissingRepackRight = 23, "missing-repack-right";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_inspection_result::MaintenanceInspectionResultCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceInspectionResultCode::ALL.len(), 23);
        assert_eq!(MaintenanceInspectionResultCode::Started.code(), 1);
        assert_eq!(MaintenanceInspectionResultCode::Cancelled.code(), 20);
        assert_eq!(MaintenanceInspectionResultCode::MissingPgRepack.code(), 21);
        assert_eq!(MaintenanceInspectionResultCode::MissingRepackRight.code(), 23);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceInspectionResultCode::try_from(0).is_err());
        assert!(MaintenanceInspectionResultCode::try_from(24).is_err());
    }
}
