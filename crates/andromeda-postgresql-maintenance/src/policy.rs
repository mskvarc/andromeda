//! Pure profile thresholds and maintenance decision policy.

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
    /// Maximum permitted active backend ratio percentage.
    pub maximum_active_backend_percent: u8,
}

impl PolicyProfile {
    /// Expand one named profile into its fixed values.
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
    pub pending_action: bool,
    /// Successful-action cooldown deadline.
    pub cooldown_until: Option<DateTime<Utc>>,
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
    /// The target remains in its successful-action cooldown.
    Cooldown,
    /// Too many database backends are currently active.
    ActiveBackendRatio,
    /// A transaction has remained open for more than 30 seconds.
    LongTransaction,
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
    if gates.active_maintenance {
        return Some(DeferralReason::ActiveMaintenance);
    }
    if gates.waiting_relation_lock {
        return Some(DeferralReason::WaitingRelationLock);
    }
    None
}

/// Apply fixed thresholds, consecutive evidence, cooldown and load gates.
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
    if gates.active_backend_percent > profile.maximum_active_backend_percent {
        return PolicyDecision::Deferred(DeferralReason::ActiveBackendRatio);
    }
    if gates.oldest_transaction_seconds > 30 {
        return PolicyDecision::Deferred(DeferralReason::LongTransaction);
    }
    if gates.active_maintenance {
        return PolicyDecision::Deferred(DeferralReason::ActiveMaintenance);
    }
    if gates.waiting_relation_lock {
        return PolicyDecision::Deferred(DeferralReason::WaitingRelationLock);
    }
    if state.pending_action {
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

#[cfg(test)]
mod tests {
    use crate::policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, SafetyGates, TargetState, decide};
    use andromeda_postgresql_context_store::maintenance_config::{MaintenanceMode, MaintenanceProfile};
    use chrono::{Duration as ChronoDuration, Utc};

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
            pending_action: true,
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
                        pending_action: true,
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
}
