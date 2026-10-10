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
    /// How long a target must breach every threshold, over at least
    /// [`MINIMUM_BREACH_SAMPLES`] samples, before work is queued.
    pub qualifying_duration: ChronoDuration,
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
                qualifying_duration: ChronoDuration::minutes(10),
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
                qualifying_duration: ChronoDuration::minutes(3),
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

/// Fewest samples a breach run needs to qualify, however long it spans: one
/// sample is a single reading, never evidence of a lasting state.
pub const MINIMUM_BREACH_SAMPLES: u32 = 2;

/// An unbroken run of samples that met every degradation threshold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BreachRun {
    /// When the run's first sample was taken.
    pub since: DateTime<Utc>,
    /// Samples in the run, at least one.
    pub samples: u32,
}

impl BreachRun {
    /// The run once a sample taken at `now` breaches too: the open run
    /// lengthened, or a new run starting at `now`.
    #[must_use]
    pub const fn extended(run: Option<BreachRun>, now: DateTime<Utc>) -> BreachRun {
        match run {
            Some(run) => BreachRun {
                since: run.since,
                samples: run.samples.saturating_add(1),
            },
            None => BreachRun { since: now, samples: 1 },
        }
    }

    /// Whether the run has lasted the profile's qualifying duration, inclusive,
    /// over at least [`MINIMUM_BREACH_SAMPLES`] samples.
    #[must_use]
    pub fn qualifies(self, profile: PolicyProfile, now: DateTime<Utc>) -> bool {
        self.samples >= MINIMUM_BREACH_SAMPLES && now.signed_duration_since(self.since) >= profile.qualifying_duration
    }
}

/// A persisted target's decision-relevant state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetState {
    /// The open breach run, `None` when the last sample did not breach or an
    /// action has completed since.
    pub breach: Option<BreachRun>,
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
    /// The target has not yet breached for the profile's qualifying duration
    /// over enough samples.
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

/// Apply fixed thresholds, breach duration, cooldown and load gates.
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
    if !BreachRun::extended(state.breach, now).qualifies(profile, now) {
        return PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples);
    }
    match mode {
        MaintenanceMode::Disabled | MaintenanceMode::Observe => PolicyDecision::WouldAct,
        MaintenanceMode::Automatic => PolicyDecision::Queued,
    }
}

/// The breach run to persist after `decision`, judged on a sample taken at
/// `now` (`sample` is `None` when a pending action was decided on without
/// one).
///
/// A breaching sample lengthens the run and a sample under any threshold
/// closes it. A decision taken before the sample was judged (cooldown, a load
/// gate, a pending action) leaves the run as it was.
#[must_use]
pub fn breach_after(breach: Option<BreachRun>, decision: PolicyDecision, sample: Option<DegradationSample>, now: DateTime<Utc>) -> Option<BreachRun> {
    match decision {
        PolicyDecision::Queued | PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples) => Some(BreachRun::extended(breach, now)),
        PolicyDecision::WouldAct => sample.map_or(breach, |_| Some(BreachRun::extended(breach, now))),
        PolicyDecision::Deferred(DeferralReason::RelationTooSmall | DeferralReason::ReclaimTooSmall | DeferralReason::BelowThreshold) => None,
        PolicyDecision::Act
        | PolicyDecision::Deferred(
            DeferralReason::Cooldown
            | DeferralReason::ActiveBackendRatio
            | DeferralReason::LongTransaction
            | DeferralReason::HorizonHeld
            | DeferralReason::ActiveMaintenance
            | DeferralReason::WaitingRelationLock,
        ) => breach,
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

/// Age at and past which a database is closer to wraparound than to its
/// freeze maximum: about half of the 2^31 transaction IDs a database may span.
const CRITICAL_FREEZE_AGE: u64 = 1_000_000_000;

/// One ID space's age in a database beside the server's freeze maximum for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FreezeAge {
    /// Age of the database's oldest unfrozen ID (`datfrozenxid` or
    /// `datminmxid`).
    pub(crate) age: u64,
    /// The age at which autovacuum forces an anti-wraparound vacuum
    /// (`autovacuum_freeze_max_age` or `autovacuum_multixact_freeze_max_age`).
    pub(crate) freeze_max_age: u64,
}

/// How far a database's anti-wraparound vacuum is behind.
///
/// Ordered from least to most urgent, so the more urgent of two pressures is
/// their maximum.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum FreezePressure {
    /// Below 125% of the freeze maximum: autovacuum is still within its window.
    Normal,
    /// At or past 125% of the freeze maximum: anti-wraparound vacuum has had a
    /// quarter of its window and not completed.
    Overdue,
    /// At or past one billion: about half of the wraparound limit is spent.
    Critical,
}

/// Classify a database by the more urgent of its transaction ID and multixact
/// ages. Both bounds are inclusive.
pub(crate) fn freeze_pressure(transaction: FreezeAge, multixact: FreezeAge) -> FreezePressure {
    Ord::max(id_space_pressure(transaction), id_space_pressure(multixact))
}

/// Classify one ID space's age against its freeze maximum.
const fn id_space_pressure(age: FreezeAge) -> FreezePressure {
    if age.age >= CRITICAL_FREEZE_AGE {
        return FreezePressure::Critical;
    }
    if age.age >= age.freeze_max_age.saturating_mul(5) / 4 {
        return FreezePressure::Overdue;
    }
    FreezePressure::Normal
}

#[cfg(test)]
mod tests {
    use crate::{
        action_measurement::{ActionComparison, TupleCounts},
        policy::{
            ActionEnding,
            ActionVerdict,
            BreachRun,
            DeferralReason,
            DegradationSample,
            FreezeAge,
            FreezePressure,
            PolicyDecision,
            PolicyProfile,
            SafetyGates,
            TargetState,
            action_verdict,
            breach_after,
            cooldown_after,
            decide,
            freeze_pressure,
        },
        vacuum_horizon::HorizonHolderKind,
    };
    use andromeda_postgresql_context_store::maintenance_config::{MaintenanceMode, MaintenanceProfile};
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
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
        assert_eq!(conservative.qualifying_duration, ChronoDuration::minutes(10));
        assert_eq!(conservative.index_free_percent, 50);
        assert_eq!(conservative.heap_dead_percent, 20);
        assert_eq!(responsive.cadence.as_secs(), 15);
        assert_eq!(responsive.maximum_jitter.as_secs(), 5);
        assert_eq!(responsive.qualifying_duration, ChronoDuration::minutes(3));
        assert_eq!(responsive.index_free_percent, 35);
        assert_eq!(responsive.heap_dead_percent, 10);
    }

    /// A target whose single breaching sample was taken `minutes` ago.
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
    fn every_threshold_is_inclusive_and_a_reset_sample_is_deferred() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let state = breached_minutes_ago(3, now);
        assert_eq!(
            decide(MaintenanceMode::Automatic, profile, state, qualifying(profile), idle_gates(), now),
            PolicyDecision::Queued
        );
        let below = DegradationSample {
            relation_bytes: profile.minimum_relation_bytes,
            reclaimable_bytes: profile.minimum_reclaim_bytes,
            degraded_percent: profile.index_free_percent - 1,
            threshold_percent: profile.index_free_percent,
        };
        assert_eq!(
            decide(MaintenanceMode::Automatic, profile, state, below, idle_gates(), now),
            PolicyDecision::Deferred(DeferralReason::BelowThreshold)
        );
    }

    #[test]
    fn observe_mode_reports_but_never_authorizes_mutation() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        assert_eq!(
            decide(
                MaintenanceMode::Observe,
                profile,
                breached_minutes_ago(3, now),
                qualifying(profile),
                idle_gates(),
                now
            ),
            PolicyDecision::WouldAct
        );
    }

    #[test]
    fn one_sample_never_qualifies() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        assert_eq!(
            decide(
                MaintenanceMode::Automatic,
                profile,
                TargetState::default(),
                qualifying(profile),
                idle_gates(),
                now
            ),
            PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)
        );
        assert!(
            !BreachRun {
                since: now - ChronoDuration::hours(1),
                samples: 1
            }
            .qualifies(profile, now)
        );
    }

    #[test]
    fn two_samples_inside_the_duration_do_not_qualify() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        assert_eq!(
            decide(
                MaintenanceMode::Automatic,
                profile,
                breached_minutes_ago(2, now),
                qualifying(profile),
                idle_gates(),
                now
            ),
            PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)
        );
    }

    #[test]
    fn two_samples_spanning_the_duration_qualify() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        assert_eq!(
            decide(
                MaintenanceMode::Automatic,
                profile,
                breached_minutes_ago(3, now),
                qualifying(profile),
                idle_gates(),
                now
            ),
            PolicyDecision::Queued
        );
        let conservative = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        assert_eq!(
            decide(
                MaintenanceMode::Automatic,
                conservative,
                breached_minutes_ago(9, now),
                qualifying(conservative),
                idle_gates(),
                now
            ),
            PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)
        );
        assert_eq!(
            decide(
                MaintenanceMode::Automatic,
                conservative,
                breached_minutes_ago(10, now),
                qualifying(conservative),
                idle_gates(),
                now
            ),
            PolicyDecision::Queued
        );
    }

    #[test]
    fn a_sample_below_threshold_clears_the_breach() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let state = breached_minutes_ago(2, now);
        let below = DegradationSample {
            degraded_percent: profile.index_free_percent - 1,
            ..qualifying(profile)
        };
        let decision = decide(MaintenanceMode::Automatic, profile, state, below, idle_gates(), now);
        assert_eq!(decision, PolicyDecision::Deferred(DeferralReason::BelowThreshold));
        assert_eq!(breach_after(state.breach, decision, Some(below), now), None);
    }

    #[test]
    fn a_breaching_sample_lengthens_the_run_and_a_gated_one_leaves_it() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let state = breached_minutes_ago(2, now);
        let sample = qualifying(profile);
        let decision = decide(MaintenanceMode::Automatic, profile, state, sample, idle_gates(), now);
        assert_eq!(
            breach_after(state.breach, decision, Some(sample), now),
            Some(BreachRun {
                since: now - ChronoDuration::minutes(2),
                samples: 2,
            })
        );
        assert_eq!(breach_after(None, decision, Some(sample), now), Some(BreachRun { since: now, samples: 1 }));
        let gated = PolicyDecision::Deferred(DeferralReason::LongTransaction);
        assert_eq!(breach_after(state.breach, gated, Some(sample), now), state.breach);
        assert_eq!(breach_after(state.breach, PolicyDecision::WouldAct, None, now), state.breach);
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

    #[test]
    fn freeze_pressure_boundaries_are_inclusive() {
        let transaction = |age| FreezeAge {
            age,
            freeze_max_age: 200_000_000,
        };
        let multixact = FreezeAge {
            age: 0,
            freeze_max_age: 400_000_000,
        };
        for (age, pressure) in [
            (249_999_999, FreezePressure::Normal),
            (250_000_000, FreezePressure::Overdue),
            (999_999_999, FreezePressure::Overdue),
            (1_000_000_000, FreezePressure::Critical),
        ] {
            assert_eq!(freeze_pressure(transaction(age), multixact), pressure, "age {age}");
        }
    }

    #[test]
    fn multixact_pressure_wins_when_higher() {
        let transaction = FreezeAge {
            age: 1_000,
            freeze_max_age: 200_000_000,
        };
        let multixact = FreezeAge {
            age: 500_000_000,
            freeze_max_age: 400_000_000,
        };
        assert_eq!(freeze_pressure(transaction, multixact), FreezePressure::Overdue);
    }
}
