//! Pure profile thresholds and maintenance decision policy.

use crate::{action_measurement::ActionComparison, vacuum_horizon::HorizonHolderKind};
use andromeda_postgresql_context_store::maintenance_config::{MaintenanceMode, MaintenanceProfile};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::time::Duration;

/// Expanded thresholds for one fixed profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProfile {
    /// Scheduled pass cadence.
    pub cadence: Duration,
    /// Upper bound for deterministic task jitter.
    pub maximum_jitter: Duration,
    /// Consecutive qualifying observations required before queuing work.
    pub qualifying_samples: u32,
    /// Small relations are never probed or maintained.
    pub minimum_relation_bytes: u64,
    /// Minimum estimated bytes maintenance must reclaim.
    pub minimum_reclaim_bytes: u64,
    /// Minimum measured index free space percentage.
    pub index_free_percent: u8,
    /// Minimum estimated heap dead space percentage.
    pub heap_dead_percent: u8,
    /// Successful-action cooldown.
    pub successful_cooldown: ChronoDuration,
    /// Cooldown after an ineffective vacuum, and the base of the failure
    /// backoff.
    pub ineffective_cooldown: ChronoDuration,
    /// Maximum permitted active backend ratio percentage.
    pub maximum_active_backend_percent: u8,
}

impl PolicyProfile {
    /// Expand one named profile into its fixed values.
    #[must_use]
    pub const fn from_profile(profile: MaintenanceProfile) -> PolicyProfile {
        const MIB: u64 = 1_048_576;
        match profile {
            MaintenanceProfile::Conservative => PolicyProfile {
                cadence: Duration::from_secs(60),
                maximum_jitter: Duration::from_secs(15),
                qualifying_samples: 3,
                minimum_relation_bytes: 64 * MIB,
                minimum_reclaim_bytes: 64 * MIB,
                index_free_percent: 50,
                heap_dead_percent: 20,
                successful_cooldown: ChronoDuration::hours(24),
                ineffective_cooldown: ChronoDuration::hours(1),
                maximum_active_backend_percent: 25,
            },
            MaintenanceProfile::Responsive => PolicyProfile {
                cadence: Duration::from_secs(15),
                maximum_jitter: Duration::from_secs(5),
                qualifying_samples: 2,
                minimum_relation_bytes: 16 * MIB,
                minimum_reclaim_bytes: 16 * MIB,
                index_free_percent: 35,
                heap_dead_percent: 10,
                successful_cooldown: ChronoDuration::hours(6),
                ineffective_cooldown: ChronoDuration::minutes(15),
                maximum_active_backend_percent: 50,
            },
        }
    }
}

/// Safety information measured immediately before an expensive operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SafetyGates {
    /// Current active backend ratio as an integer percentage.
    pub active_backend_percent: u8,
    /// Age in whole seconds of the oldest open transaction.
    pub oldest_transaction_seconds: u64,
    /// Kind of the first vacuum horizon holder that will not release the
    /// horizon on its own, `None` when none does or the target is an index.
    pub held_horizon: Option<HorizonHolderKind>,
    /// Whether a concurrent index build or vacuum already targets the relation.
    pub active_maintenance: bool,
    /// Whether another backend waits for a lock on the relation.
    pub waiting_relation_lock: bool,
}

/// A persisted target's decision-relevant state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetState {
    /// Number of immediately preceding qualifying samples.
    pub consecutive_breaches: u32,
    /// Work already qualified on an earlier pass.
    pub is_action_pending: bool,
    /// Cooldown deadline after an action, absent before the first action.
    pub cooldown_until: Option<DateTime<Utc>>,
    /// Number of immediately preceding failed actions.
    pub consecutive_failures: u32,
}

/// Why a pass deliberately declined expensive work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferralReason {
    /// The fixed profile excludes the relation size.
    RelationTooSmall,
    /// The fixed profile excludes the estimated reclaim.
    ReclaimTooSmall,
    /// The measured degradation percentage is below threshold.
    BelowThreshold,
    /// More consecutive qualifying samples are required.
    ConsecutiveSamples,
    /// The target remains in the cooldown its last action set.
    Cooldown,
    /// Too many database backends are currently active.
    ActiveBackendRatio,
    /// A transaction has remained open for more than 30 seconds.
    LongTransaction,
    /// A prepared transaction or an inactive replication slot holds the
    /// vacuum horizon, so a vacuum could remove nothing it protects.
    HorizonHeld,
    /// PostgreSQL already reports a build or vacuum on the target.
    ActiveMaintenance,
    /// Another backend is waiting for a relation lock.
    WaitingRelationLock,
}

/// The policy's exact result for one target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    /// Automatic mode may execute the already-pending operation.
    Act,
    /// Automatic mode persisted newly qualified work for a later pass.
    Queued,
    /// Observe mode reports that automatic mode would act.
    WouldAct,
    /// No action is currently justified.
    Deferred(DeferralReason),
}

/// One degradation sample compared with policy thresholds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DegradationSample {
    /// Physical relation bytes.
    pub relation_bytes: u64,
    /// Estimated reclaimable bytes.
    pub reclaimable_bytes: u64,
    /// Index free-space or heap dead-space percentage.
    pub degraded_percent: u8,
    /// Percentage threshold selected for this target class.
    pub threshold_percent: u8,
}

impl DegradationSample {
    /// Return the first threshold this sample does not satisfy.
    ///
    /// # Errors
    ///
    /// Returns [`DeferralReason::RelationTooSmall`] when the relation is below
    /// the profile's minimum size, [`DeferralReason::ReclaimTooSmall`] when too
    /// few bytes would be reclaimed, and [`DeferralReason::BelowThreshold`] when
    /// the measured degradation has not reached the target's threshold. The
    /// thresholds are tested in that order, so only the first unmet one is
    /// reported.
    pub const fn qualification(self, profile: PolicyProfile) -> Result<(), DeferralReason> {
        if self.relation_bytes < profile.minimum_relation_bytes {
            return Err(DeferralReason::RelationTooSmall);
        }
        if self.reclaimable_bytes < profile.minimum_reclaim_bytes {
            return Err(DeferralReason::ReclaimTooSmall);
        }
        if self.degraded_percent < self.threshold_percent {
            return Err(DeferralReason::BelowThreshold);
        }
        Ok(())
    }
}

/// Return the first active safety gate in policy order.
pub(crate) const fn safety_deferral(profile: PolicyProfile, gates: SafetyGates) -> Option<DeferralReason> {
    if gates.active_backend_percent > profile.maximum_active_backend_percent {
        return Some(DeferralReason::ActiveBackendRatio);
    }
    if gates.oldest_transaction_seconds > 30 {
        return Some(DeferralReason::LongTransaction);
    }
    if gates.held_horizon.is_some() {
        return Some(DeferralReason::HorizonHeld);
    }
    if gates.active_maintenance {
        return Some(DeferralReason::ActiveMaintenance);
    }
    if gates.waiting_relation_lock {
        return Some(DeferralReason::WaitingRelationLock);
    }
    None
}

/// Apply fixed thresholds, consecutive evidence, cooldown and load gates.
#[must_use]
pub fn decide(
    mode: MaintenanceMode,
    profile: PolicyProfile,
    state: TargetState,
    sample: DegradationSample,
    gates: SafetyGates,
    now: DateTime<Utc>,
) -> PolicyDecision {
    if let Some(until) = state.cooldown_until
        && until > now
    {
        return PolicyDecision::Deferred(DeferralReason::Cooldown);
    }
    if let Some(reason) = safety_deferral(profile, gates) {
        return PolicyDecision::Deferred(reason);
    }
    if state.is_action_pending {
        return match mode {
            MaintenanceMode::Disabled | MaintenanceMode::Observe => PolicyDecision::WouldAct,
            MaintenanceMode::Automatic => PolicyDecision::Act,
        };
    }
    if let Err(reason) = sample.qualification(profile) {
        return PolicyDecision::Deferred(reason);
    }
    if state.consecutive_breaches.saturating_add(1) < profile.qualifying_samples {
        return PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples);
    }
    match mode {
        MaintenanceMode::Disabled | MaintenanceMode::Observe => PolicyDecision::WouldAct,
        MaintenanceMode::Automatic => PolicyDecision::Queued,
    }
}

/// Whether a completed action achieved what it was queued for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionVerdict {
    /// The action achieved its purpose.
    Effective,
    /// A vacuum completed, yet the dead share it left still meets the
    /// threshold that queued it: something held the horizon.
    Ineffective,
}

/// Judge a completed action by the reading taken after it.
///
/// A rebuild is always effective, and so is a vacuum of a heap left empty.
pub(crate) fn action_verdict(profile: PolicyProfile, comparison: ActionComparison) -> ActionVerdict {
    match comparison.post_vacuum_dead_percent() {
        Some(dead_percent) if dead_percent >= profile.heap_dead_percent => ActionVerdict::Ineffective,
        Some(_) | None => ActionVerdict::Effective,
    }
}

/// How one executed action ended, as the cooldown reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionEnding {
    /// The statement completed.
    Completed(ActionVerdict),
    /// The statement failed.
    Failed {
        /// Consecutive failures including this one.
        consecutive_failures: u32,
    },
}

/// The cooldown deadline an action's ending sets on its target.
///
/// A failure backs off exponentially from the ineffective cooldown, doubling
/// with each consecutive failure up to 24 hours, so a rebuild that keeps
/// failing stops leaving an artifact to recover on every visit.
#[must_use]
pub fn cooldown_after(profile: PolicyProfile, ending: ActionEnding, now: DateTime<Utc>) -> DateTime<Utc> {
    now + match ending {
        ActionEnding::Completed(ActionVerdict::Effective) => profile.successful_cooldown,
        ActionEnding::Completed(ActionVerdict::Ineffective) => profile.ineffective_cooldown,
        ActionEnding::Failed { consecutive_failures } => failure_backoff(profile.ineffective_cooldown, consecutive_failures),
    }
}

/// `base * 2^(consecutive_failures - 1)`, capped at 24 hours. A product too
/// large to represent is past the cap.
fn failure_backoff(base: ChronoDuration, consecutive_failures: u32) -> ChronoDuration {
    let cap = ChronoDuration::hours(24);
    2_i32
        .checked_pow(consecutive_failures.saturating_sub(1))
        .and_then(|factor| base.checked_mul(factor))
        .map_or(cap, |backoff| backoff.min(cap))
}

#[cfg(test)]
mod tests {
    use crate::{
        action_measurement::{ActionComparison, TupleCounts},
        policy::{
            ActionEnding,
            ActionVerdict,
            DeferralReason,
            DegradationSample,
            PolicyDecision,
            PolicyProfile,
            SafetyGates,
            TargetState,
            action_verdict,
            cooldown_after,
            decide,
        },
        vacuum_horizon::HorizonHolderKind,
    };
    use andromeda_postgresql_context_store::maintenance_config::{MaintenanceMode, MaintenanceProfile};
    use chrono::{Duration as ChronoDuration, Utc};
    use pretty_assertions::assert_eq;

    fn qualifying(profile: PolicyProfile) -> DegradationSample {
        DegradationSample {
            relation_bytes: profile.minimum_relation_bytes,
            reclaimable_bytes: profile.minimum_reclaim_bytes,
            degraded_percent: profile.index_free_percent,
            threshold_percent: profile.index_free_percent,
        }
    }

    const fn idle_gates() -> SafetyGates {
        SafetyGates {
            active_backend_percent: 0,
            oldest_transaction_seconds: 0,
            held_horizon: None,
            active_maintenance: false,
            waiting_relation_lock: false,
        }
    }

    #[test]
    fn profile_values_match_the_operator_contract() {
        let conservative = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let responsive = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        assert_eq!(conservative.cadence.as_secs(), 60);
        assert_eq!(conservative.maximum_jitter.as_secs(), 15);
        assert_eq!(conservative.qualifying_samples, 3);
        assert_eq!(conservative.index_free_percent, 50);
        assert_eq!(conservative.heap_dead_percent, 20);
        assert_eq!(responsive.cadence.as_secs(), 15);
        assert_eq!(responsive.maximum_jitter.as_secs(), 5);
        assert_eq!(responsive.qualifying_samples, 2);
        assert_eq!(responsive.index_free_percent, 35);
        assert_eq!(responsive.heap_dead_percent, 10);
    }

    #[test]
    fn every_threshold_is_inclusive_and_a_reset_sample_is_deferred() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let state = TargetState {
            consecutive_breaches: 1,
            ..TargetState::default()
        };
        assert_eq!(
            decide(MaintenanceMode::Automatic, profile, state, qualifying(profile), idle_gates(), Utc::now()),
            PolicyDecision::Queued
        );
        let below = DegradationSample {
            relation_bytes: profile.minimum_relation_bytes,
            reclaimable_bytes: profile.minimum_reclaim_bytes,
            degraded_percent: profile.index_free_percent - 1,
            threshold_percent: profile.index_free_percent,
        };
        assert_eq!(
            decide(MaintenanceMode::Automatic, profile, state, below, idle_gates(), Utc::now()),
            PolicyDecision::Deferred(DeferralReason::BelowThreshold)
        );
    }

    #[test]
    fn observe_mode_reports_but_never_authorizes_mutation() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let state = TargetState {
            consecutive_breaches: 1,
            ..TargetState::default()
        };
        assert_eq!(
            decide(MaintenanceMode::Observe, profile, state, qualifying(profile), idle_gates(), Utc::now()),
            PolicyDecision::WouldAct
        );
    }

    #[test]
    fn successful_cooldown_and_each_load_gate_defer() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let sample = qualifying(profile);
        let cooldown = TargetState {
            is_action_pending: true,
            cooldown_until: Some(now + ChronoDuration::hours(1)),
            ..TargetState::default()
        };
        assert_eq!(
            decide(MaintenanceMode::Automatic, profile, cooldown, sample, idle_gates(), now),
            PolicyDecision::Deferred(DeferralReason::Cooldown)
        );
        for (gates, reason) in [
            (
                SafetyGates {
                    active_backend_percent: 51,
                    ..idle_gates()
                },
                DeferralReason::ActiveBackendRatio,
            ),
            (
                SafetyGates {
                    oldest_transaction_seconds: 31,
                    ..idle_gates()
                },
                DeferralReason::LongTransaction,
            ),
            (
                SafetyGates {
                    held_horizon: Some(HorizonHolderKind::PreparedTransaction),
                    ..idle_gates()
                },
                DeferralReason::HorizonHeld,
            ),
            (
                SafetyGates {
                    active_maintenance: true,
                    ..idle_gates()
                },
                DeferralReason::ActiveMaintenance,
            ),
            (
                SafetyGates {
                    waiting_relation_lock: true,
                    ..idle_gates()
                },
                DeferralReason::WaitingRelationLock,
            ),
        ] {
            assert_eq!(
                decide(
                    MaintenanceMode::Automatic,
                    profile,
                    TargetState {
                        is_action_pending: true,
                        ..TargetState::default()
                    },
                    sample,
                    gates,
                    now,
                ),
                PolicyDecision::Deferred(reason)
            );
        }
    }

    #[test]
    fn vacuum_is_ineffective_when_the_dead_share_still_meets_the_threshold() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let vacuum = |dead, live| ActionComparison::Heap {
            before: TupleCounts { dead: 50_000, live },
            after: TupleCounts { dead, live },
        };
        assert_eq!(profile.heap_dead_percent, 10);
        assert_eq!(action_verdict(profile, vacuum(20, 80)), ActionVerdict::Ineffective);
        assert_eq!(action_verdict(profile, vacuum(5, 95)), ActionVerdict::Effective);
        assert_eq!(action_verdict(profile, vacuum(0, 0)), ActionVerdict::Effective);
    }

    #[test]
    fn ineffective_vacuum_takes_the_short_cooldown() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        assert_eq!(
            cooldown_after(profile, ActionEnding::Completed(ActionVerdict::Ineffective), now),
            now + ChronoDuration::minutes(15)
        );
        assert_eq!(
            cooldown_after(profile, ActionEnding::Completed(ActionVerdict::Effective), now),
            now + ChronoDuration::hours(6)
        );
        assert_eq!(
            PolicyProfile::from_profile(MaintenanceProfile::Conservative).ineffective_cooldown,
            ChronoDuration::hours(1)
        );
    }

    #[test]
    fn failed_action_backs_off_exponentially() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        for (consecutive_failures, minutes) in [(1, 15), (2, 30), (3, 60)] {
            assert_eq!(
                cooldown_after(profile, ActionEnding::Failed { consecutive_failures }, now),
                now + ChronoDuration::minutes(minutes)
            );
        }
    }

    #[test]
    fn failure_backoff_caps_at_twenty_four_hours() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        for (consecutive_failures, hours) in [(7, 16), (8, 24), (40, 24)] {
            assert_eq!(
                cooldown_after(profile, ActionEnding::Failed { consecutive_failures }, now),
                now + ChronoDuration::hours(hours)
            );
        }
    }
}
