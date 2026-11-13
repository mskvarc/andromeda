//! The persisted policy decision of one maintenance target.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The policy decision last recorded for a maintenance target.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case.
    MaintenanceDecisionCode {
        frozen_prefix: 21,
        frozen_digest: 0x65cd_5cc5_c0e7_e1a9,
        codes: {
            /// Act now.
            Act = 1, "act";
            /// Queue the action.
            Queued = 2, "queued";
            /// An action is due but acting is disabled.
            WouldAct = 3, "would-act";
            /// Deferred: the relation is below the size floor.
            RelationTooSmall = 4, "relation-too-small";
            /// Deferred: too little space would be reclaimed.
            ReclaimTooSmall = 5, "reclaim-too-small";
            /// Deferred: degradation is below the threshold.
            BelowThreshold = 6, "below-threshold";
            /// Deferred: too few consecutive breaching samples.
            ConsecutiveSamples = 7, "consecutive-samples";
            /// Deferred: the target is cooling down.
            Cooldown = 8, "cooldown";
            /// Deferred: the server is too busy.
            ActiveBackendRatio = 9, "active-backend-ratio";
            /// Deferred: a long transaction is open.
            LongTransaction = 10, "long-transaction";
            /// Deferred: other maintenance is running.
            ActiveMaintenance = 11, "active-maintenance";
            /// Deferred: a relation lock is awaited.
            WaitingRelationLock = 12, "waiting-relation-lock";
            /// Queued together with its paired index.
            PairedQueued = 13, "paired-queued";
            /// Deferred: a holder that never advances on its own holds the
            /// vacuum horizon.
            HorizonHeld = 14, "horizon-held";
            /// Deferred: a heap's dead tuples wait for vacuum before its free
            /// space is worth a repack.
            VacuumPending = 15, "vacuum-pending";
            /// Deferred: the relation is above the largest a repack copies.
            RelationTooLarge = 16, "relation-too-large";
            /// Deferred: a replication slot keeps more WAL than the relation
            /// takes, and a repack writes the relation's size again.
            ReplicationRetention = 17, "replication-retention";
            /// Deferred: the Snapshot whose branch holds the relation expires
            /// before a repack of it could finish.
            SnapshotLifetimeShort = 18, "snapshot-lifetime-short";
            /// Deferred: objects an earlier repack left wait for their cleanup.
            RepackLeftover = 19, "repack-leftover";
            /// Deferred: the database's anti-wraparound vacuum is behind, and a
            /// repack's copy would hold its freeze horizon back.
            FreezePressure = 20, "freeze-pressure";
            /// Deferred: a repack of the relation is running.
            RepackRunning = 21, "repack-running";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_decision::MaintenanceDecisionCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceDecisionCode::ALL.len(), 21);
        assert_eq!(MaintenanceDecisionCode::Act.code(), 1);
        assert_eq!(MaintenanceDecisionCode::PairedQueued.code(), 13);
        assert_eq!(MaintenanceDecisionCode::HorizonHeld.code(), 14);
        assert_eq!(MaintenanceDecisionCode::VacuumPending.code(), 15);
        assert_eq!(MaintenanceDecisionCode::RepackRunning.code(), 21);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceDecisionCode::try_from(0).is_err());
        assert!(MaintenanceDecisionCode::try_from(22).is_err());
    }
}
