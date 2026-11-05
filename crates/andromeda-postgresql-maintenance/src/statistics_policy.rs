//! How a heap's planner statistics qualify for `ANALYZE`, and when an
//! analyze already queued has lapsed. Each takes the cooldown and load gates
//! every target takes first.

use crate::{
    policy::{DeferralReason, PolicyDecision, PolicyProfile, SafetyGates, TargetState, held, newly_qualified, pending_decision},
    statistics_freshness::StatisticsFreshness,
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use chrono::{DateTime, Utc};

/// Apply cooldown and load gates to a heap's statistics, which qualify for
/// `ANALYZE` on one stale sample: an analyze reads a bounded sample, so
/// neither a breach duration nor the size floors apply.
///
/// A queued analyze is judged on freshness too, unlike other queued work:
/// fresh statistics leave it nothing to do, so it is deferred as
/// [`DeferralReason::BelowThreshold`] and [`lapse`](crate::policy::lapse) withdraws it.
pub(crate) fn decide_analyze(
    mode: MaintenanceMode,
    profile: PolicyProfile,
    state: TargetState,
    freshness: StatisticsFreshness,
    gates: SafetyGates,
    now: DateTime<Utc>,
) -> PolicyDecision {
    if let Some(reason) = held(profile, state, gates, now) {
        return PolicyDecision::Deferred(reason);
    }
    match freshness {
        StatisticsFreshness::Stale if state.is_action_pending => pending_decision(mode),
        StatisticsFreshness::Stale => newly_qualified(mode),
        StatisticsFreshness::Fresh => PolicyDecision::Deferred(DeferralReason::BelowThreshold),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        policy::{DeferralReason, PolicyDecision, PolicyProfile, SafetyGates, TargetState, lapse},
        statistics_freshness::StatisticsFreshness,
        statistics_policy::decide_analyze,
    };
    use andromeda_configuration::schema::database::maintenance::{MaintenanceMode, MaintenanceProfile};
    use chrono::{Duration as ChronoDuration, Utc};
    use pretty_assertions::assert_eq;

    const fn idle_gates() -> SafetyGates {
        SafetyGates {
            active_backend_percent: 0,
            oldest_transaction_seconds: 0,
            held_horizon: None,
            active_maintenance: false,
            waiting_relation_lock: false,
            repack_running: false,
        }
    }

    fn queued() -> TargetState {
        TargetState {
            is_action_pending: true,
            ..TargetState::default()
        }
    }

    /// An analyze reads a bounded sample, so one stale sample of a heap of any
    /// size qualifies it, while cooldown and load gates still defer it.
    #[test]
    fn stale_statistics_qualify_on_one_sample_within_the_gates() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let now = Utc::now();
        let fresh_state = TargetState::default();
        assert_eq!(
            decide_analyze(MaintenanceMode::Automatic, profile, fresh_state, StatisticsFreshness::Stale, idle_gates(), now),
            PolicyDecision::Queued
        );
        assert_eq!(
            decide_analyze(MaintenanceMode::Observe, profile, fresh_state, StatisticsFreshness::Stale, idle_gates(), now),
            PolicyDecision::WouldAct
        );
        assert_eq!(
            decide_analyze(MaintenanceMode::Automatic, profile, fresh_state, StatisticsFreshness::Fresh, idle_gates(), now),
            PolicyDecision::Deferred(DeferralReason::BelowThreshold)
        );
        let cooling = TargetState {
            cooldown_until: Some(now + ChronoDuration::minutes(30)),
            ..TargetState::default()
        };
        assert_eq!(
            decide_analyze(MaintenanceMode::Automatic, profile, cooling, StatisticsFreshness::Stale, idle_gates(), now),
            PolicyDecision::Deferred(DeferralReason::Cooldown)
        );
        let busy = SafetyGates {
            active_maintenance: true,
            ..idle_gates()
        };
        assert_eq!(
            decide_analyze(MaintenanceMode::Automatic, profile, fresh_state, StatisticsFreshness::Stale, busy, now),
            PolicyDecision::Deferred(DeferralReason::ActiveMaintenance)
        );
    }

    #[test]
    fn a_queued_analyze_of_still_stale_statistics_is_acted_on() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let decision = decide_analyze(
            MaintenanceMode::Automatic,
            profile,
            queued(),
            StatisticsFreshness::Stale,
            idle_gates(),
            Utc::now(),
        );

        assert_eq!(decision, PolicyDecision::Act);
        assert_eq!(lapse(queued(), decision), None);
    }

    #[test]
    fn a_queued_analyze_of_statistics_analyzed_since_lapses_instead_of_acting() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let decision = decide_analyze(
            MaintenanceMode::Automatic,
            profile,
            queued(),
            StatisticsFreshness::Fresh,
            idle_gates(),
            Utc::now(),
        );

        assert_eq!(decision, PolicyDecision::Deferred(DeferralReason::BelowThreshold));
        assert_eq!(lapse(queued(), decision), Some(DeferralReason::BelowThreshold));
    }

    #[test]
    fn a_queued_analyze_in_its_cooldown_or_behind_a_load_gate_keeps_its_deferral_and_stays_queued() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let now = Utc::now();
        let cooling = TargetState {
            cooldown_until: Some(now + ChronoDuration::minutes(30)),
            ..queued()
        };
        let busy = SafetyGates {
            oldest_transaction_seconds: 31,
            ..idle_gates()
        };

        for freshness in [StatisticsFreshness::Stale, StatisticsFreshness::Fresh] {
            let in_cooldown = decide_analyze(MaintenanceMode::Automatic, profile, cooling, freshness, idle_gates(), now);
            let gated = decide_analyze(MaintenanceMode::Automatic, profile, queued(), freshness, busy, now);
            assert_eq!(in_cooldown, PolicyDecision::Deferred(DeferralReason::Cooldown), "{freshness:?}");
            assert_eq!(gated, PolicyDecision::Deferred(DeferralReason::LongTransaction), "{freshness:?}");
            assert_eq!((lapse(cooling, in_cooldown), lapse(queued(), gated)), (None, None), "{freshness:?}");
        }
    }

    #[test]
    fn a_queued_analyze_under_observe_mode_would_act_while_stale_and_lapses_once_fresh() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let now = Utc::now();

        assert_eq!(
            decide_analyze(MaintenanceMode::Observe, profile, queued(), StatisticsFreshness::Stale, idle_gates(), now),
            PolicyDecision::WouldAct
        );
        assert_eq!(
            decide_analyze(MaintenanceMode::Observe, profile, queued(), StatisticsFreshness::Fresh, idle_gates(), now),
            PolicyDecision::Deferred(DeferralReason::BelowThreshold)
        );
    }
}
