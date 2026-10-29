//! How the history hypertables' chunk targets qualify: a partially converted
//! chunk for its recompression, a closed rowstore chunk for its rebuild.
//! Each takes the cooldown and load gates every target takes first.

use crate::{
    columnstore_partiality::ChunkPartiality,
    policy::{BreachRun, DeferralReason, PolicyDecision, PolicyProfile, SafetyGates, TargetState, gated_decision, newly_qualified},
    rowstore_rebuild::RebuildAwaiting,
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use chrono::{DateTime, Utc};

/// Apply cooldown, load gates and the breach duration to a history
/// hypertable's partial chunks, which qualify for recompression once one has
/// stayed partial for the profile's qualifying duration over at least
/// [`MINIMUM_BREACH_SAMPLES`](crate::policy::MINIMUM_BREACH_SAMPLES) samples, so a burst of corrections is
/// recompressed once. The size floors do not apply: recompression rewrites
/// only the segments a write reached, so its cost follows the writes, not the
/// chunk.
pub(crate) fn decide_recompression(
    mode: MaintenanceMode,
    profile: PolicyProfile,
    state: TargetState,
    partiality: ChunkPartiality,
    gates: SafetyGates,
    now: DateTime<Utc>,
) -> PolicyDecision {
    if let Some(decision) = gated_decision(mode, profile, state, gates, now) {
        return decision;
    }
    match partiality {
        ChunkPartiality::Whole => PolicyDecision::Deferred(DeferralReason::BelowThreshold),
        ChunkPartiality::Partial if BreachRun::extended(state.breach, now).qualifies(profile, now) => newly_qualified(mode),
        ChunkPartiality::Partial => PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples),
    }
}

/// Apply cooldown and load gates to a history hypertable's closed rowstore
/// chunks, which qualify for their rebuild on one sample: a closed chunk's
/// indexes do not change between samples, so a breach duration would only
/// delay the rebuild, and the candidate already holds the relation floor.
pub(crate) fn decide_rowstore_rebuild(
    mode: MaintenanceMode,
    profile: PolicyProfile,
    state: TargetState,
    awaiting: RebuildAwaiting,
    gates: SafetyGates,
    now: DateTime<Utc>,
) -> PolicyDecision {
    if let Some(decision) = gated_decision(mode, profile, state, gates, now) {
        return decision;
    }
    match awaiting {
        RebuildAwaiting::Awaiting => newly_qualified(mode),
        RebuildAwaiting::None => PolicyDecision::Deferred(DeferralReason::BelowThreshold),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_partiality::ChunkPartiality,
        history_chunk_policy::{decide_recompression, decide_rowstore_rebuild},
        policy::{BreachRun, DeferralReason, PolicyDecision, PolicyProfile, SafetyGates, TargetState, breach_after},
        rowstore_rebuild::RebuildAwaiting,
    };
    use andromeda_configuration::schema::database::maintenance::{MaintenanceMode, MaintenanceProfile};
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use pretty_assertions::assert_eq;

    const fn idle_gates() -> SafetyGates {
        SafetyGates {
            active_backend_percent: 0,
            oldest_transaction_seconds: 0,
            held_horizon: None,
            active_maintenance: false,
            waiting_relation_lock: false,
        }
    }

    fn breached_minutes_ago(minutes: i64, now: DateTime<Utc>) -> TargetState {
        TargetState {
            breach: Some(BreachRun {
                since: now - ChronoDuration::minutes(minutes),
                samples: 1,
            }),
            ..TargetState::default()
        }
    }

    #[test]
    fn a_partial_chunk_reports_would_act_in_observe_mode() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let now = Utc::now();
        assert_eq!(
            decide_recompression(
                MaintenanceMode::Observe,
                profile,
                breached_minutes_ago(10, now),
                ChunkPartiality::Partial,
                idle_gates(),
                now
            ),
            PolicyDecision::WouldAct
        );
    }

    #[test]
    fn a_whole_columnstore_closes_the_breach_and_gates_still_defer_a_pending_recompression() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let state = breached_minutes_ago(5, now);
        let decision = decide_recompression(MaintenanceMode::Automatic, profile, state, ChunkPartiality::Whole, idle_gates(), now);
        assert_eq!(decision, PolicyDecision::Deferred(DeferralReason::BelowThreshold));
        assert_eq!(breach_after(state.breach, decision, None, now), None);
        let pending = TargetState {
            is_action_pending: true,
            ..TargetState::default()
        };
        let locked = SafetyGates {
            waiting_relation_lock: true,
            ..idle_gates()
        };
        assert_eq!(
            decide_recompression(MaintenanceMode::Automatic, profile, pending, ChunkPartiality::Partial, locked, now),
            PolicyDecision::Deferred(DeferralReason::WaitingRelationLock)
        );
        assert_eq!(
            decide_recompression(MaintenanceMode::Automatic, profile, pending, ChunkPartiality::Partial, idle_gates(), now),
            PolicyDecision::Act
        );
    }

    /// A closed chunk's indexes do not change between samples, so the first
    /// sample that finds one awaiting its rebuild queues it, with no breach
    /// run; observe mode only reports it.
    #[test]
    fn a_chunk_awaiting_its_rebuild_qualifies_on_one_sample() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let now = Utc::now();
        let fresh = TargetState::default();
        assert_eq!(
            decide_rowstore_rebuild(MaintenanceMode::Automatic, profile, fresh, RebuildAwaiting::Awaiting, idle_gates(), now),
            PolicyDecision::Queued
        );
        assert_eq!(
            decide_rowstore_rebuild(MaintenanceMode::Observe, profile, fresh, RebuildAwaiting::Awaiting, idle_gates(), now),
            PolicyDecision::WouldAct
        );
        assert_eq!(
            decide_rowstore_rebuild(MaintenanceMode::Automatic, profile, fresh, RebuildAwaiting::None, idle_gates(), now),
            PolicyDecision::Deferred(DeferralReason::BelowThreshold)
        );
    }

    #[test]
    fn a_pending_chunk_rebuild_acts_unless_a_gate_defers_it() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let pending = TargetState {
            is_action_pending: true,
            ..TargetState::default()
        };
        let locked = SafetyGates {
            waiting_relation_lock: true,
            ..idle_gates()
        };
        assert_eq!(
            decide_rowstore_rebuild(MaintenanceMode::Automatic, profile, pending, RebuildAwaiting::Awaiting, locked, now),
            PolicyDecision::Deferred(DeferralReason::WaitingRelationLock)
        );
        assert_eq!(
            decide_rowstore_rebuild(MaintenanceMode::Automatic, profile, pending, RebuildAwaiting::Awaiting, idle_gates(), now),
            PolicyDecision::Act
        );
    }

    #[test]
    fn a_partial_chunk_qualifies_for_recompression_once_partial_for_the_qualifying_duration() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let recompression = |state| decide_recompression(MaintenanceMode::Automatic, profile, state, ChunkPartiality::Partial, idle_gates(), now);
        assert_eq!(
            recompression(TargetState::default()),
            PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)
        );
        assert_eq!(
            recompression(breached_minutes_ago(2, now)),
            PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)
        );
        assert_eq!(recompression(breached_minutes_ago(3, now)), PolicyDecision::Queued);
        let decision = recompression(TargetState::default());
        assert_eq!(breach_after(None, decision, None, now), Some(BreachRun { since: now, samples: 1 }));
    }
}
