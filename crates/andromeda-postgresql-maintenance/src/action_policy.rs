//! How a completed or failed maintenance action is judged, and how long its
//! target cools down afterwards.

use crate::{action_measurement::ActionComparison, candidate::MaintenanceTarget, columnstore_partiality::ChunkPartiality, policy::PolicyProfile};
use chrono::{DateTime, Duration as ChronoDuration, Utc};

/// Cooldown after a completed `ANALYZE`, whatever the profile. An analyze
/// reads a bounded sample, so it is cheap enough to repeat within the hour
/// autoanalyze keeps missing a heap.
const ANALYZE_COOLDOWN: ChronoDuration = ChronoDuration::hours(1);

/// Cooldown after a completed recompression, whatever the profile: none. It
/// recompresses the oldest partial chunk only, and the next partial chunk
/// opens a breach run of its own, which already spaces recompressions by the
/// qualifying duration.
const RECOMPRESSION_COOLDOWN: ChronoDuration = ChronoDuration::zero();

/// Whether a completed action achieved what it was queued for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionVerdict {
    /// The action achieved its purpose.
    Effective,
    /// A vacuum left the dead share at or above the threshold that queued it,
    /// because something held the horizon; a rebuild of a density-measured
    /// GIN reclaimed less than the reclaim floor, because its density grew
    /// with its values rather than with bloat; or a write reached a chunk
    /// while it was recompressed and left it partial.
    Ineffective,
}

/// Judge a completed action by the reading taken after it.
///
/// A vacuum of a heap left empty is effective. A rebuild of an index probed or
/// queued through a canary is always effective: its free space was measured,
/// not inferred. A completed analyze is effective, and so is a recompression
/// that left its chunk whole.
pub(crate) fn action_verdict(profile: PolicyProfile, target: MaintenanceTarget, comparison: ActionComparison) -> ActionVerdict {
    match comparison {
        ActionComparison::Index { before_bytes, after_bytes } => match target {
            MaintenanceTarget::Index(index) if index.density_column().is_some() && before_bytes.saturating_sub(after_bytes) < profile.minimum_reclaim_bytes => {
                ActionVerdict::Ineffective
            }
            MaintenanceTarget::Index(_) | MaintenanceTarget::Heap(_) | MaintenanceTarget::Statistics(_) | MaintenanceTarget::Columnstore(_) => {
                ActionVerdict::Effective
            }
        },
        ActionComparison::Heap { after, .. } => match after.dead_percent() {
            Some(dead_percent) if dead_percent >= profile.heap_dead_percent => ActionVerdict::Ineffective,
            Some(_) | None => ActionVerdict::Effective,
        },
        ActionComparison::Statistics { .. } => ActionVerdict::Effective,
        ActionComparison::Columnstore { after } => match after {
            ChunkPartiality::Whole => ActionVerdict::Effective,
            ChunkPartiality::Partial => ActionVerdict::Ineffective,
        },
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
/// A completed analyze cools down for [`ANALYZE_COOLDOWN`] and a completed
/// recompression for [`RECOMPRESSION_COOLDOWN`] in every profile. A failure
/// backs off exponentially from the ineffective cooldown, doubling with each
/// consecutive failure up to 24 hours, so a rebuild that keeps failing stops
/// leaving an artifact to recover on every visit.
#[must_use]
pub fn cooldown_after(profile: PolicyProfile, target: MaintenanceTarget, ending: ActionEnding, now: DateTime<Utc>) -> DateTime<Utc> {
    now + match (target, ending) {
        (MaintenanceTarget::Statistics(_), ActionEnding::Completed(_)) => ANALYZE_COOLDOWN,
        (MaintenanceTarget::Columnstore(_), ActionEnding::Completed(_)) => RECOMPRESSION_COOLDOWN,
        (MaintenanceTarget::Index(_) | MaintenanceTarget::Heap(_), ActionEnding::Completed(ActionVerdict::Effective)) => profile.successful_cooldown,
        (MaintenanceTarget::Index(_) | MaintenanceTarget::Heap(_), ActionEnding::Completed(ActionVerdict::Ineffective)) => profile.ineffective_cooldown,
        (
            MaintenanceTarget::Index(_) | MaintenanceTarget::Heap(_) | MaintenanceTarget::Statistics(_) | MaintenanceTarget::Columnstore(_),
            ActionEnding::Failed { consecutive_failures },
        ) => failure_backoff(profile.ineffective_cooldown, consecutive_failures),
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
        action_policy::{ActionEnding, ActionVerdict, action_verdict, cooldown_after},
        candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable},
        columnstore_partiality::ChunkPartiality,
        policy::PolicyProfile,
    };
    use andromeda_postgresql_context_store::maintenance_config::MaintenanceProfile;
    use chrono::{Duration as ChronoDuration, Utc};
    use pretty_assertions::assert_eq;

    const VACUUM: MaintenanceTarget = MaintenanceTarget::Heap(HeapTarget::CurrentAttributes);

    #[test]
    fn vacuum_is_ineffective_when_the_dead_share_still_meets_the_threshold() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let vacuum = |dead, live| ActionComparison::Heap {
            before: TupleCounts { dead: 50_000, live },
            after: TupleCounts { dead, live },
        };
        let heap = MaintenanceTarget::Heap(HeapTarget::CurrentAttributes);
        assert_eq!(profile.heap_dead_percent, 10);
        assert_eq!(action_verdict(profile, heap, vacuum(20, 80)), ActionVerdict::Ineffective);
        assert_eq!(action_verdict(profile, heap, vacuum(5, 95)), ActionVerdict::Effective);
        assert_eq!(action_verdict(profile, heap, vacuum(0, 0)), ActionVerdict::Effective);
    }

    #[test]
    fn ineffective_vacuum_takes_the_short_cooldown() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        assert_eq!(
            cooldown_after(profile, VACUUM, ActionEnding::Completed(ActionVerdict::Ineffective), now),
            now + ChronoDuration::minutes(15)
        );
        assert_eq!(
            cooldown_after(profile, VACUUM, ActionEnding::Completed(ActionVerdict::Effective), now),
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
                cooldown_after(profile, VACUUM, ActionEnding::Failed { consecutive_failures }, now),
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
                cooldown_after(profile, VACUUM, ActionEnding::Failed { consecutive_failures }, now),
                now + ChronoDuration::hours(hours)
            );
        }
    }

    #[test]
    fn a_completed_analyze_cools_down_for_one_hour_in_every_profile() {
        let analyze = MaintenanceTarget::Statistics(HeapTarget::CurrentAttributes);
        let now = Utc::now();
        for profile in [MaintenanceProfile::Conservative, MaintenanceProfile::Responsive] {
            let profile = PolicyProfile::from_profile(profile);
            assert_eq!(
                cooldown_after(profile, analyze, ActionEnding::Completed(ActionVerdict::Effective), now),
                now + ChronoDuration::hours(1)
            );
            assert_eq!(
                cooldown_after(profile, analyze, ActionEnding::Failed { consecutive_failures: 2 }, now),
                now + profile.ineffective_cooldown * 2
            );
        }
    }

    #[test]
    fn a_recompression_is_ineffective_when_a_write_left_its_chunk_partial_and_never_cools_down() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Conservative);
        let recompression = MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory);
        let after = |after| ActionComparison::Columnstore { after };
        assert_eq!(action_verdict(profile, recompression, after(ChunkPartiality::Whole)), ActionVerdict::Effective);
        assert_eq!(
            action_verdict(profile, recompression, after(ChunkPartiality::Partial)),
            ActionVerdict::Ineffective
        );
        let now = Utc::now();
        for verdict in [ActionVerdict::Effective, ActionVerdict::Ineffective] {
            assert_eq!(cooldown_after(profile, recompression, ActionEnding::Completed(verdict), now), now);
        }
        assert_eq!(
            cooldown_after(profile, recompression, ActionEnding::Failed { consecutive_failures: 1 }, now),
            now + profile.ineffective_cooldown
        );
    }
}
