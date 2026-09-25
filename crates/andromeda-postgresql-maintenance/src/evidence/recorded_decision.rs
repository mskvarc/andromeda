//! The decision last recorded for one target.

use crate::policy::{DeferralReason, PolicyDecision};
use andromeda_postgresql_codes::maintenance_decision::MaintenanceDecisionCode;

/// The decision `postgresql_maintenance_targets.last_decision` records.
///
/// Every policy decision, plus the one the controller makes on the policy's
/// behalf: queueing the trigram index together with its string canary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordedDecision {
    /// The policy's own decision for the target.
    Policy(PolicyDecision),
    /// Queued together with its paired string index.
    PairedQueued,
}

impl RecordedDecision {
    /// The registry member the platform database stores for this decision.
    pub(crate) const fn code(self) -> MaintenanceDecisionCode {
        match self {
            RecordedDecision::Policy(PolicyDecision::Act) => MaintenanceDecisionCode::Act,
            RecordedDecision::Policy(PolicyDecision::Queued) => MaintenanceDecisionCode::Queued,
            RecordedDecision::Policy(PolicyDecision::WouldAct) => MaintenanceDecisionCode::WouldAct,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::RelationTooSmall)) => MaintenanceDecisionCode::RelationTooSmall,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ReclaimTooSmall)) => MaintenanceDecisionCode::ReclaimTooSmall,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::BelowThreshold)) => MaintenanceDecisionCode::BelowThreshold,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)) => MaintenanceDecisionCode::ConsecutiveSamples,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::Cooldown)) => MaintenanceDecisionCode::Cooldown,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ActiveBackendRatio)) => MaintenanceDecisionCode::ActiveBackendRatio,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::LongTransaction)) => MaintenanceDecisionCode::LongTransaction,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ActiveMaintenance)) => MaintenanceDecisionCode::ActiveMaintenance,
            RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::WaitingRelationLock)) => MaintenanceDecisionCode::WaitingRelationLock,
            RecordedDecision::PairedQueued => MaintenanceDecisionCode::PairedQueued,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        evidence::recorded_decision::RecordedDecision,
        policy::{DeferralReason, PolicyDecision},
    };
    use andromeda_postgresql_codes::maintenance_decision::MaintenanceDecisionCode;
    use pretty_assertions::assert_eq;

    /// Every decision the controller can record, in registry order.
    const EVERY_DECISION: [RecordedDecision; 13] = [
        RecordedDecision::Policy(PolicyDecision::Act),
        RecordedDecision::Policy(PolicyDecision::Queued),
        RecordedDecision::Policy(PolicyDecision::WouldAct),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::RelationTooSmall)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ReclaimTooSmall)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::BelowThreshold)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::Cooldown)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ActiveBackendRatio)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::LongTransaction)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::ActiveMaintenance)),
        RecordedDecision::Policy(PolicyDecision::Deferred(DeferralReason::WaitingRelationLock)),
        RecordedDecision::PairedQueued,
    ];

    /// The mapping is one-to-one and onto: every decision has its own number
    /// and every number the registry assigns is some decision's.
    #[test]
    fn every_recorded_decision_has_its_own_stored_number_and_every_number_is_used() {
        let stored: Vec<MaintenanceDecisionCode> = EVERY_DECISION.iter().map(|decision| decision.code()).collect();
        assert_eq!(stored, MaintenanceDecisionCode::ALL.to_vec());
    }
}
