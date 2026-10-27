//! Pure profile thresholds and maintenance decision policy.

use crate::{columnstore_partiality::ChunkPartiality, statistics_freshness::StatisticsFreshness, vacuum_horizon::HorizonHolderKind};
use andromeda_configuration::schema::database::maintenance::{MaintenanceMode, MaintenanceProfile};
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::time::Duration;

/// The longest cooldown a run of failed actions backs off to, so a rebuild
/// that keeps failing is still retried once a day.
pub const FAILURE_BACKOFF_CEILING: ChronoDuration = ChronoDuration::hours(24);

/// Ineffective-vacuum cooldown of the conservative profile.
const CONSERVATIVE_INEFFECTIVE_COOLDOWN: ChronoDuration = ChronoDuration::hours(1);

/// Ineffective-vacuum cooldown of the responsive profile.
const RESPONSIVE_INEFFECTIVE_COOLDOWN: ChronoDuration = ChronoDuration::minutes(15);

/// The failure backoff doubling from `ineffective` to
/// [`FAILURE_BACKOFF_CEILING`]; both are whole seconds.
const fn failure_backoff_from(ineffective: ChronoDuration) -> BackoffSchedule {
    BackoffSchedule::new(
        Duration::from_secs(ineffective.num_seconds().unsigned_abs()),
        Duration::from_secs(FAILURE_BACKOFF_CEILING.num_seconds().unsigned_abs()),
        Growth::Doubling,
    )
}

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
    /// Cooldown after an ineffective vacuum.
    pub ineffective_cooldown: ChronoDuration,
    /// Cooldown after a failed action: doubling from the ineffective cooldown
    /// with each consecutive failure, up to [`FAILURE_BACKOFF_CEILING`].
    pub failure_backoff: BackoffSchedule,
    /// Maximum permitted active backend ratio percentage.
    pub maximum_active_backend_percent: u8,
    /// Wall time one visit may spend on detailed index probes.
    pub probe_budget: Duration,
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
                ineffective_cooldown: CONSERVATIVE_INEFFECTIVE_COOLDOWN,
                failure_backoff: failure_backoff_from(CONSERVATIVE_INEFFECTIVE_COOLDOWN),
                maximum_active_backend_percent: 25,
                probe_budget: Duration::from_secs(60),
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
                ineffective_cooldown: RESPONSIVE_INEFFECTIVE_COOLDOWN,
                failure_backoff: failure_backoff_from(RESPONSIVE_INEFFECTIVE_COOLDOWN),
                maximum_active_backend_percent: 50,
                probe_budget: Duration::from_secs(30),
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
    /// The measured degradation percentage is below threshold, or a heap's
    /// statistics are fresh.
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
    if let Some(decision) = gated_decision(mode, profile, state, gates, now) {
        return decision;
    }
    if let Err(reason) = sample.qualification(profile) {
        return PolicyDecision::Deferred(reason);
    }
    if !BreachRun::extended(state.breach, now).qualifies(profile, now) {
        return PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples);
    }
    newly_qualified(mode)
}

/// Apply cooldown and load gates to a heap's statistics, which qualify for
/// `ANALYZE` on one stale sample: an analyze reads a bounded sample, so
/// neither a breach duration nor the size floors apply.
pub(crate) fn decide_analyze(
    mode: MaintenanceMode,
    profile: PolicyProfile,
    state: TargetState,
    freshness: StatisticsFreshness,
    gates: SafetyGates,
    now: DateTime<Utc>,
) -> PolicyDecision {
    if let Some(decision) = gated_decision(mode, profile, state, gates, now) {
        return decision;
    }
    match freshness {
        StatisticsFreshness::Stale => newly_qualified(mode),
        StatisticsFreshness::Fresh => PolicyDecision::Deferred(DeferralReason::BelowThreshold),
    }
}

/// Apply cooldown, load gates and the breach duration to a history
/// hypertable's partial chunks, which qualify for recompression once one has
/// stayed partial for the profile's qualifying duration over at least
/// [`MINIMUM_BREACH_SAMPLES`] samples, so a burst of corrections is
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

/// The decision every target takes before its own qualification: deferred in
/// its cooldown or by a load gate, and acted on once pending. `None` leaves
/// the target to its qualification.
fn gated_decision(mode: MaintenanceMode, profile: PolicyProfile, state: TargetState, gates: SafetyGates, now: DateTime<Utc>) -> Option<PolicyDecision> {
    if let Some(until) = state.cooldown_until
        && until > now
    {
        return Some(PolicyDecision::Deferred(DeferralReason::Cooldown));
    }
    if let Some(reason) = safety_deferral(profile, gates) {
        return Some(PolicyDecision::Deferred(reason));
    }
    state.is_action_pending.then_some(match mode {
        MaintenanceMode::Disabled | MaintenanceMode::Observe => PolicyDecision::WouldAct,
        MaintenanceMode::Automatic => PolicyDecision::Act,
    })
}

/// The decision on work that qualified on this sample.
const fn newly_qualified(mode: MaintenanceMode) -> PolicyDecision {
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

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_partiality::ChunkPartiality,
        policy::{
            BreachRun,
            DeferralReason,
            DegradationSample,
            PolicyDecision,
            PolicyProfile,
            SafetyGates,
            TargetState,
            breach_after,
            decide,
            decide_analyze,
            decide_recompression,
        },
        statistics_freshness::StatisticsFreshness,
        vacuum_horizon::HorizonHolderKind,
    };
    use andromeda_configuration::schema::database::maintenance::{MaintenanceMode, MaintenanceProfile};
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
        assert_eq!(conservative.probe_budget.as_secs(), 60);
        assert_eq!(responsive.cadence.as_secs(), 15);
        assert_eq!(responsive.maximum_jitter.as_secs(), 5);
        assert_eq!(responsive.qualifying_duration, ChronoDuration::minutes(3));
        assert_eq!(responsive.index_free_percent, 35);
        assert_eq!(responsive.heap_dead_percent, 10);
        assert_eq!(responsive.probe_budget.as_secs(), 30);
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
        let pending = TargetState {
            is_action_pending: true,
            ..TargetState::default()
        };
        assert_eq!(
            decide_analyze(MaintenanceMode::Automatic, profile, pending, StatisticsFreshness::Fresh, idle_gates(), now),
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
}
