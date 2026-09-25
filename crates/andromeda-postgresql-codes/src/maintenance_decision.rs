//! The persisted policy decision of one maintenance target.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The policy decision last recorded for a maintenance target.
    ///
    /// An Andromeda operational concept, so the labels are kebab-case.
    MaintenanceDecisionCode {
        frozen_prefix: 13,
        frozen_digest: 0xf84e_598d_3383_1243,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_decision::MaintenanceDecisionCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceDecisionCode::ALL.len(), 13);
        assert_eq!(MaintenanceDecisionCode::Act.code(), 1);
        assert_eq!(MaintenanceDecisionCode::PairedQueued.code(), 13);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceDecisionCode::try_from(0).is_err());
        assert!(MaintenanceDecisionCode::try_from(14).is_err());
    }
}
