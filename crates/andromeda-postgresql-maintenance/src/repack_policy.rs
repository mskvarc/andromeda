//! Whether a heap's free space is worth an online repack.
//!
//! Vacuum makes dead tuples reusable but never gives a heap's pages back: a
//! queue a burst filled and drained keeps its size until it is rewritten. A
//! repack rewrites it online, which costs a full copy, its indexes built
//! again, the relation's size in WAL, and free disk of up to twice the
//! relation. So it qualifies only on free space that lasts: the qualifying
//! duration is long enough that a queue which drains and refills on its own
//! never stays above the free-space threshold for all of it.
//!
//! The thresholds are tested in a fixed order, so only the first unmet one is
//! reported: the size floor, the configured size ceiling, dead tuples vacuum
//! has still to turn into free space, the free-space share, then the bytes a
//! repack would give back. Before any of them the target's cooldown, the
//! load gates and the repack's own gates decide: a run of the table already
//! in progress or left behind, a replication slot keeping more WAL than the
//! relation takes, a Snapshot branch expiring before the run could finish,
//! and freeze pressure, since the copy holds the database's freeze horizon
//! for as long as it reads.

use crate::{
    branch_selection::EXPIRY_MARGIN,
    breach_run::BreachRun,
    catalog_counter::percent,
    freeze_policy::FreezePressure,
    heap_free_space::FreeSpaceReading,
    policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, SafetyGates, TargetState, held, newly_qualified, pending_decision},
    repack::phase_budget::phase_budget,
};
use andromeda_configuration::schema::database::maintenance::{MaintenanceMode, RepackPolicy};
use chrono::{DateTime, Duration as ChronoDuration, Utc};

/// The thresholds one profile qualifies a repack on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepackThresholds {
    /// The smallest relation, heap and TOAST together, a repack is weighed
    /// for.
    pub relation_floor_bytes: u64,
    /// The fewest bytes a repack must give back.
    pub reclaim_floor_bytes: u64,
    /// The smallest free-space share, as a percentage of the relation.
    pub free_percent: u8,
    /// How long the relation must breach every threshold, over at least two
    /// samples, before a repack is queued.
    pub qualifying_duration: ChronoDuration,
    /// The cooldown after a repack that gave back what it was queued for.
    pub successful_cooldown: ChronoDuration,
}

impl RepackThresholds {
    /// The conservative profile's thresholds.
    pub const CONSERVATIVE: RepackThresholds = RepackThresholds {
        relation_floor_bytes: 256 * MIB,
        reclaim_floor_bytes: 128 * MIB,
        free_percent: 50,
        qualifying_duration: ChronoDuration::hours(24),
        successful_cooldown: ChronoDuration::days(7),
    };

    /// The responsive profile's thresholds.
    pub const RESPONSIVE: RepackThresholds = RepackThresholds {
        relation_floor_bytes: 64 * MIB,
        reclaim_floor_bytes: 32 * MIB,
        free_percent: 35,
        qualifying_duration: ChronoDuration::hours(6),
        successful_cooldown: ChronoDuration::days(2),
    };
}

/// One mebibyte.
const MIB: u64 = 1_048_576;

/// One heap's free space, its TOAST table's included, as a repack weighs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RepackSample {
    /// The relation's bytes, its free bytes and their share, against the
    /// profile's free-space threshold.
    pub(crate) degradation: DegradationSample,
    /// The heap's dead tuples, as a percentage of its pages.
    pub(crate) dead_percent: u8,
}

impl RepackSample {
    /// The sample `reading` makes against `profile`'s repack threshold.
    pub(crate) fn of_reading(reading: FreeSpaceReading, profile: PolicyProfile) -> RepackSample {
        RepackSample::new(reading.relation_bytes, reading.free_bytes, reading.dead_percent, profile)
    }

    /// The sample of a relation of `relation_bytes` with `free_bytes` free and
    /// `dead_percent` dead, against `profile`'s repack threshold.
    pub(crate) fn new(relation_bytes: u64, free_bytes: u64, dead_percent: u8, profile: PolicyProfile) -> RepackSample {
        RepackSample {
            degradation: DegradationSample {
                relation_bytes,
                reclaimable_bytes: free_bytes,
                degraded_percent: percent(free_bytes, relation_bytes),
                threshold_percent: profile.repack.free_percent,
            },
            dead_percent,
        }
    }

    /// The first threshold the sample does not meet, tested in the order the
    /// module states.
    ///
    /// # Errors
    ///
    /// Returns the deferral naming the first unmet threshold:
    /// [`DeferralReason::RelationTooSmall`],
    /// [`DeferralReason::RelationTooLarge`] above `max_relation_bytes`,
    /// [`DeferralReason::VacuumPending`] at or above the profile's heap dead
    /// share, [`DeferralReason::BelowThreshold`] or
    /// [`DeferralReason::ReclaimTooSmall`].
    pub(crate) const fn qualification(self, profile: PolicyProfile, max_relation_bytes: Option<u64>) -> Result<(), DeferralReason> {
        let thresholds = profile.repack;
        let relation_bytes = self.degradation.relation_bytes;
        if relation_bytes < thresholds.relation_floor_bytes {
            return Err(DeferralReason::RelationTooSmall);
        }
        if let Some(max) = max_relation_bytes
            && relation_bytes > max
        {
            return Err(DeferralReason::RelationTooLarge);
        }
        if self.dead_percent >= profile.heap_dead_percent {
            return Err(DeferralReason::VacuumPending);
        }
        if self.degradation.degraded_percent < thresholds.free_percent {
            return Err(DeferralReason::BelowThreshold);
        }
        if self.degradation.reclaimable_bytes < thresholds.reclaim_floor_bytes {
            return Err(DeferralReason::ReclaimTooSmall);
        }
        Ok(())
    }
}

/// What a repack's run state in the tenant database is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepackRunState {
    /// No run's objects are present for the table.
    Idle,
    /// A run of the table holds its lock and is copying it.
    Running,
    /// A run's objects are present and no run holds the table's lock: they
    /// wait for their cleanup.
    Leftover,
}

/// What decides a repack beyond its sample and the load gates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RepackContext {
    /// The configured largest relation a repack copies, `None` when unlimited.
    pub(crate) max_relation_bytes: Option<u64>,
    /// The most WAL any replication slot keeps on disk.
    pub(crate) retained_wal_bytes: u64,
    /// How long the Snapshot whose branch holds the relation still lives,
    /// `None` for the live branch.
    pub(crate) snapshot_remaining: Option<ChronoDuration>,
    /// The database's freeze pressure.
    pub(crate) freeze: FreezePressure,
    /// Whether a run of the table is present.
    pub(crate) run: RepackRunState,
}

impl RepackContext {
    /// The repack's own gate on a relation of `relation_bytes`, `None` when
    /// none holds it back.
    fn gate(self, relation_bytes: u64) -> Option<DeferralReason> {
        match self.run {
            RepackRunState::Running => return Some(DeferralReason::RepackRunning),
            RepackRunState::Leftover => return Some(DeferralReason::RepackLeftover),
            RepackRunState::Idle => {}
        }
        if self.retained_wal_bytes > relation_bytes {
            return Some(DeferralReason::ReplicationRetention);
        }
        if let Some(remaining) = self.snapshot_remaining
            && remaining < run_lifetime(relation_bytes)
        {
            return Some(DeferralReason::SnapshotLifetimeShort);
        }
        if self.freeze > FreezePressure::Normal {
            return Some(DeferralReason::FreezePressure);
        }
        None
    }
}

/// The lifetime a Snapshot branch must still have for a repack of a relation
/// of `relation_bytes` to run in it: the run's phase budget and the margin
/// every maintenance statement in a Snapshot branch keeps.
fn run_lifetime(relation_bytes: u64) -> ChronoDuration {
    ChronoDuration::from_std(phase_budget(relation_bytes)).map_or(ChronoDuration::MAX, |budget| budget + EXPIRY_MARGIN)
}

/// The mode a repack is decided under: the controller's, except that an
/// automatic controller only reports repacks unless they are performed.
pub(crate) const fn repack_mode(controller: MaintenanceMode, policy: RepackPolicy) -> MaintenanceMode {
    match (controller, policy) {
        (MaintenanceMode::Disabled, RepackPolicy::Reported | RepackPolicy::Performed) => MaintenanceMode::Disabled,
        (MaintenanceMode::Observe, RepackPolicy::Reported | RepackPolicy::Performed) | (MaintenanceMode::Automatic, RepackPolicy::Reported) => {
            MaintenanceMode::Observe
        }
        (MaintenanceMode::Automatic, RepackPolicy::Performed) => MaintenanceMode::Automatic,
    }
}

/// Decide a repack of one heap: its cooldown and the load gates, its own
/// gates, its sample's thresholds, then its breach run over the repack's
/// qualifying duration. A repack already queued is acted on once no gate
/// holds it back, and judged again on a fresh sample just before it runs.
pub(crate) fn decide_repack(
    mode: MaintenanceMode,
    profile: PolicyProfile,
    state: TargetState,
    sample: RepackSample,
    context: RepackContext,
    gates: SafetyGates,
    now: DateTime<Utc>,
) -> PolicyDecision {
    // The trigger the gate reads is a repack's own; the run state says
    // whether it belongs to a running run or to one left behind.
    let gates = SafetyGates {
        repack_running: false,
        ..gates
    };
    if let Some(reason) = held(profile, state, gates, now).or_else(|| context.gate(sample.degradation.relation_bytes)) {
        return PolicyDecision::Deferred(reason);
    }
    if state.is_action_pending {
        return pending_decision(mode);
    }
    if let Err(reason) = sample.qualification(profile, context.max_relation_bytes) {
        return PolicyDecision::Deferred(reason);
    }
    if !BreachRun::extended(state.breach, now).qualifies(profile.repack.qualifying_duration, now) {
        return PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples);
    }
    newly_qualified(mode)
}

#[cfg(test)]
mod tests {
    use crate::{
        breach_run::BreachRun,
        freeze_policy::FreezePressure,
        policy::{DeferralReason, PolicyDecision, PolicyProfile, SafetyGates, TargetState, decide},
        repack_policy::{RepackContext, RepackRunState, RepackSample, decide_repack, repack_mode},
    };
    use andromeda_configuration::schema::database::maintenance::{MaintenanceMode, MaintenanceProfile, RepackPolicy};
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;

    const MIB: u64 = 1_048_576;
    const GIB: u64 = 1_073_741_824;

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

    /// A live-branch table nothing else holds back, with no size ceiling.
    const fn live_context() -> RepackContext {
        RepackContext {
            max_relation_bytes: Some(4 * GIB),
            retained_wal_bytes: 0,
            snapshot_remaining: None,
            freeze: FreezePressure::Normal,
            run: RepackRunState::Idle,
        }
    }

    /// A target whose first breaching sample was taken `hours` ago, the
    /// sample being decided its second.
    fn breached_hours_ago(hours: i64, now: DateTime<Utc>) -> TargetState {
        TargetState {
            breach: Some(BreachRun {
                since: now - Duration::hours(hours),
                samples: 1,
            }),
            ..TargetState::default()
        }
    }

    fn conservative() -> PolicyProfile {
        PolicyProfile::from_profile(MaintenanceProfile::Conservative)
    }

    fn responsive() -> PolicyProfile {
        PolicyProfile::from_profile(MaintenanceProfile::Responsive)
    }

    /// 512 MiB with 60% free is 307 MiB to give back.
    fn bloated(profile: PolicyProfile) -> RepackSample {
        RepackSample::new(512 * MIB, 512 * MIB * 60 / 100, 0, profile)
    }

    fn decided(mode: MaintenanceMode, profile: PolicyProfile, state: TargetState, sample: RepackSample, context: RepackContext) -> PolicyDecision {
        decide_repack(mode, profile, state, sample, context, idle_gates(), Utc::now())
    }

    #[test]
    fn the_profiles_carry_the_operator_contract() {
        let conservative = conservative().repack;
        assert_eq!(
            (conservative.relation_floor_bytes, conservative.reclaim_floor_bytes, conservative.free_percent),
            (256 * MIB, 128 * MIB, 50)
        );
        assert_eq!(
            (conservative.qualifying_duration, conservative.successful_cooldown),
            (Duration::hours(24), Duration::days(7))
        );
        let responsive = responsive().repack;
        assert_eq!(
            (responsive.relation_floor_bytes, responsive.reclaim_floor_bytes, responsive.free_percent),
            (64 * MIB, 32 * MIB, 35)
        );
        assert_eq!(
            (responsive.qualifying_duration, responsive.successful_cooldown),
            (Duration::hours(6), Duration::days(2))
        );
    }

    #[test]
    fn a_heap_free_for_a_day_is_queued_in_automatic_mode() {
        let now = Utc::now();
        let mode = repack_mode(MaintenanceMode::Automatic, RepackPolicy::Performed);
        assert_eq!(
            decide_repack(
                mode,
                conservative(),
                breached_hours_ago(24, now),
                bloated(conservative()),
                live_context(),
                idle_gates(),
                now
            ),
            PolicyDecision::Queued
        );
    }

    #[test]
    fn an_observing_controller_or_reported_repacks_only_report_the_repack() {
        let now = Utc::now();
        for (controller, repack) in [
            (MaintenanceMode::Automatic, RepackPolicy::Reported),
            (MaintenanceMode::Observe, RepackPolicy::Performed),
        ] {
            let mode = repack_mode(controller, repack);
            assert_eq!(
                decide_repack(
                    mode,
                    conservative(),
                    breached_hours_ago(24, now),
                    bloated(conservative()),
                    live_context(),
                    idle_gates(),
                    now
                ),
                PolicyDecision::WouldAct,
                "{controller:?} with {repack:?}"
            );
        }
    }

    #[test]
    fn only_an_automatic_controller_performing_repacks_runs_them() {
        for policy in [RepackPolicy::Reported, RepackPolicy::Performed] {
            assert_eq!(repack_mode(MaintenanceMode::Disabled, policy), MaintenanceMode::Disabled);
            assert_eq!(repack_mode(MaintenanceMode::Observe, policy), MaintenanceMode::Observe);
        }
        assert_eq!(repack_mode(MaintenanceMode::Automatic, RepackPolicy::Reported), MaintenanceMode::Observe);
        assert_eq!(repack_mode(MaintenanceMode::Automatic, RepackPolicy::Performed), MaintenanceMode::Automatic);
    }

    #[test]
    fn a_relation_under_the_floor_is_too_small() {
        let now = Utc::now();
        let sample = RepackSample::new(200 * MIB, 150 * MIB, 0, conservative());
        assert_eq!(
            decided(MaintenanceMode::Automatic, conservative(), breached_hours_ago(24, now), sample, live_context()),
            PolicyDecision::Deferred(DeferralReason::RelationTooSmall)
        );
    }

    /// 300 MiB with 40% free misses the 50% threshold before its 120 MiB
    /// misses the 128 MiB reclaim floor.
    #[test]
    fn a_free_share_under_the_threshold_is_below_it() {
        let now = Utc::now();
        let sample = RepackSample::new(300 * MIB, 120 * MIB, 0, conservative());
        assert_eq!(
            decided(MaintenanceMode::Automatic, conservative(), breached_hours_ago(24, now), sample, live_context()),
            PolicyDecision::Deferred(DeferralReason::BelowThreshold)
        );
    }

    /// 70 MiB with 40% free meets the responsive 35% but gives back 28 MiB,
    /// under the 32 MiB floor.
    #[test]
    fn too_few_bytes_to_give_back_is_reclaim_too_small() {
        let now = Utc::now();
        let sample = RepackSample::new(70 * MIB, 28 * MIB, 0, responsive());
        assert_eq!(
            decided(MaintenanceMode::Automatic, responsive(), breached_hours_ago(6, now), sample, live_context()),
            PolicyDecision::Deferred(DeferralReason::ReclaimTooSmall)
        );
    }

    #[test]
    fn a_relation_over_the_ceiling_is_too_large_and_an_unlimited_ceiling_admits_it() {
        let now = Utc::now();
        let sample = RepackSample::new(5 * GIB, 4 * GIB, 0, conservative());
        assert_eq!(
            decided(MaintenanceMode::Automatic, conservative(), breached_hours_ago(24, now), sample, live_context()),
            PolicyDecision::Deferred(DeferralReason::RelationTooLarge)
        );
        let unlimited = RepackContext {
            max_relation_bytes: None,
            ..live_context()
        };
        assert_eq!(
            decided(MaintenanceMode::Automatic, conservative(), breached_hours_ago(24, now), sample, unlimited),
            PolicyDecision::Queued
        );
    }

    /// 25% dead is over the conservative 20% heap threshold.
    #[test]
    fn dead_tuples_over_the_vacuum_threshold_wait_for_vacuum() {
        let now = Utc::now();
        let sample = RepackSample::new(512 * MIB, 307 * MIB, 25, conservative());
        assert_eq!(
            decided(MaintenanceMode::Automatic, conservative(), breached_hours_ago(24, now), sample, live_context()),
            PolicyDecision::Deferred(DeferralReason::VacuumPending)
        );
    }

    #[test]
    fn samples_spanning_less_than_the_qualifying_duration_wait() {
        let now = Utc::now();
        assert_eq!(
            decided(
                MaintenanceMode::Automatic,
                conservative(),
                breached_hours_ago(23, now),
                bloated(conservative()),
                live_context()
            ),
            PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples)
        );
    }

    #[test]
    fn a_repack_six_days_ago_still_cools_down() {
        let now = Utc::now();
        let state = TargetState {
            cooldown_until: Some(now + Duration::days(1)),
            ..breached_hours_ago(24, now)
        };
        assert_eq!(
            decide_repack(
                MaintenanceMode::Automatic,
                conservative(),
                state,
                bloated(conservative()),
                live_context(),
                idle_gates(),
                now
            ),
            PolicyDecision::Deferred(DeferralReason::Cooldown)
        );
    }

    /// 80 MiB with 40% free gives back 32 MiB, exactly the responsive floor.
    #[test]
    fn the_responsive_profile_queues_a_smaller_heap_after_six_hours() {
        let now = Utc::now();
        let sample = RepackSample::new(80 * MIB, 32 * MIB, 0, responsive());
        assert_eq!(
            decided(MaintenanceMode::Automatic, responsive(), breached_hours_ago(6, now), sample, live_context()),
            PolicyDecision::Queued
        );
    }

    #[test]
    fn a_slot_keeping_more_wal_than_the_relation_holds_the_repack_back() {
        let now = Utc::now();
        let context = RepackContext {
            retained_wal_bytes: 513 * MIB,
            ..live_context()
        };
        assert_eq!(
            decided(
                MaintenanceMode::Automatic,
                conservative(),
                breached_hours_ago(24, now),
                bloated(conservative()),
                context
            ),
            PolicyDecision::Deferred(DeferralReason::ReplicationRetention)
        );
    }

    /// A 512 MiB relation has the 30 minute budget floor, so its branch must
    /// live for an hour with the margin; 20 minutes is too short.
    #[test]
    fn a_snapshot_branch_expiring_before_the_run_could_finish_waits() {
        let now = Utc::now();
        let short = RepackContext {
            snapshot_remaining: Some(Duration::minutes(20)),
            ..live_context()
        };
        assert_eq!(
            decided(
                MaintenanceMode::Automatic,
                conservative(),
                breached_hours_ago(24, now),
                bloated(conservative()),
                short
            ),
            PolicyDecision::Deferred(DeferralReason::SnapshotLifetimeShort)
        );
        let long = RepackContext {
            snapshot_remaining: Some(Duration::minutes(61)),
            ..live_context()
        };
        assert_eq!(
            decided(
                MaintenanceMode::Automatic,
                conservative(),
                breached_hours_ago(24, now),
                bloated(conservative()),
                long
            ),
            PolicyDecision::Queued
        );
    }

    #[test]
    fn freeze_pressure_holds_the_repack_back() {
        let now = Utc::now();
        let context = RepackContext {
            freeze: FreezePressure::Overdue,
            ..live_context()
        };
        assert_eq!(
            decided(
                MaintenanceMode::Automatic,
                conservative(),
                breached_hours_ago(24, now),
                bloated(conservative()),
                context
            ),
            PolicyDecision::Deferred(DeferralReason::FreezePressure)
        );
    }

    #[test]
    fn a_run_in_progress_or_left_behind_holds_the_repack_back() {
        let now = Utc::now();
        for (run, reason) in [
            (RepackRunState::Running, DeferralReason::RepackRunning),
            (RepackRunState::Leftover, DeferralReason::RepackLeftover),
        ] {
            let context = RepackContext { run, ..live_context() };
            assert_eq!(
                decided(
                    MaintenanceMode::Automatic,
                    conservative(),
                    breached_hours_ago(24, now),
                    bloated(conservative()),
                    context
                ),
                PolicyDecision::Deferred(reason)
            );
        }
    }

    #[test]
    fn a_vacuum_of_a_table_being_repacked_waits_for_the_run() {
        let profile = conservative();
        let gates = SafetyGates {
            repack_running: true,
            ..idle_gates()
        };
        let queued = TargetState {
            is_action_pending: true,
            ..TargetState::default()
        };
        let sample = bloated(profile).degradation;
        assert_eq!(
            decide(MaintenanceMode::Automatic, profile, queued, sample, gates, Utc::now()),
            PolicyDecision::Deferred(DeferralReason::RepackRunning)
        );
    }

    #[test]
    fn a_queued_repack_is_acted_on_once_no_gate_holds_it_back() {
        let queued = TargetState {
            is_action_pending: true,
            ..TargetState::default()
        };
        assert_eq!(
            decided(MaintenanceMode::Automatic, conservative(), queued, bloated(conservative()), live_context()),
            PolicyDecision::Act
        );
    }
}
