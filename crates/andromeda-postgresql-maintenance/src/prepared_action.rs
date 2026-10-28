//! Resolving what one pending action works on, whether it still qualifies,
//! and the readings taken around it.
//!
//! A fixed target names its relation at compile time; a columnstore target
//! works on whichever of its chunks is the oldest partial one, read from the
//! catalogue in the visit that acts on it, never from configuration or a
//! request.
//!
//! An action qualifies on the sample that queued it and runs on a later pass,
//! when autovacuum may already have done the work. The reading taken before a
//! vacuum, an analyze or a recompression is judged again with the thresholds
//! that queued it, so a heap autovacuum caught up with, or a chunk no write
//! left partial, is not maintained a second time. A rebuild is not judged
//! again: its bloat was measured by a detailed probe, and an index does not
//! shed bloat on its own.

use crate::{
    action_measurement::{ActionBaseline, ActionComparison, TupleCounts},
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    columnstore_partiality,
    error::MaintenanceError,
    inspection::{self, HeapSample},
    policy::{DeferralReason, PolicyProfile, SafetyGates},
    safety_gate_inspection::{self, GateScope},
    statistics_freshness::{self, HeapStatistics, StatisticsFreshness},
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceCommand, recompress_chunk};
use chrono::{DateTime, Utc};
use sqlx::PgConnection;

/// What one pending action runs with once its subject is resolved: the gates
/// measured on its relations, the reading taken before it, its statement, and
/// the relation it works on, for the log.
pub(crate) struct PreparedAction {
    /// The safety gates of the action's relations.
    pub(crate) gates: SafetyGates,
    /// The measure read before the action.
    pub(crate) baseline: ActionBaseline,
    /// The fixed statement the action runs.
    pub(crate) statement: MaintenanceCommand,
    /// The relation the statement works on.
    pub(crate) relation: String,
}

/// What a pending action comes to once its subject is read again.
pub(crate) enum Preparation {
    /// The action still qualifies and runs as prepared.
    Ready(PreparedAction),
    /// The reading no longer meets the threshold that queued the action, for
    /// the reason given.
    Lapsed(DeferralReason),
}

/// Resolve what a pending action works on and judge it again on a live
/// reading. A vacuum, an analyze or a recompression that no longer qualifies
/// is [`Preparation::Lapsed`], with no gate measured; anything else measures
/// its gates and keeps the reading as its baseline.
pub(crate) async fn prepare_action(connection: &mut PgConnection, row: BranchTarget, profile: PolicyProfile) -> Result<Preparation, MaintenanceError> {
    let branch = row.branch();
    let prepared = match row.target() {
        MaintenanceTarget::Index(index) => PreparedAction {
            gates: safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?,
            baseline: ActionBaseline::Index {
                index,
                before_bytes: inspection::index_bytes(connection, branch, index).await?,
            },
            statement: index.reindex_statement(branch),
            relation: branch.qualified(index.relation_name()),
        },
        MaintenanceTarget::Heap(heap) => {
            let sample = inspection::heap_sample(connection, branch, heap, profile).await?;
            let before = match vacuum_baseline(sample, profile) {
                Ok(before) => before,
                Err(reason) => return Ok(Preparation::Lapsed(reason)),
            };
            PreparedAction {
                gates: safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?,
                baseline: ActionBaseline::Heap { heap, before },
                statement: heap.vacuum_statement(branch),
                relation: branch.qualified(heap.relation_name()),
            }
        }
        MaintenanceTarget::Statistics(heap) => {
            let statistics = statistics_freshness::heap_statistics_of_target(connection, branch, heap).await?;
            let before_modified = match analyze_baseline(statistics, Utc::now()) {
                Ok(before_modified) => before_modified,
                Err(reason) => return Ok(Preparation::Lapsed(reason)),
            };
            PreparedAction {
                gates: safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?,
                baseline: ActionBaseline::Statistics { heap, before_modified },
                statement: heap.analyze_statement(branch),
                relation: branch.qualified(heap.relation_name()),
            }
        }
        MaintenanceTarget::Columnstore(table) => {
            let Some((chunk, _)) = columnstore_partiality::oldest_partial_chunk(connection, table).await? else {
                return Ok(Preparation::Lapsed(DeferralReason::BelowThreshold));
            };
            PreparedAction {
                gates: safety_gate_inspection::safety_gates(connection, GateScope::Chunk { table, chunk: &chunk }).await?,
                baseline: ActionBaseline::Columnstore { chunk: chunk.oid },
                statement: recompress_chunk(chunk.oid),
                relation: chunk.relation,
            }
        }
    };
    Ok(Preparation::Ready(prepared))
}

/// The tuple counts a queued vacuum starts from, or the first threshold the
/// heap's live sample no longer meets.
fn vacuum_baseline(sample: HeapSample, profile: PolicyProfile) -> Result<TupleCounts, DeferralReason> {
    sample.degradation.qualification(profile).map(|()| sample.counts)
}

/// The modified tuples a queued analyze starts from, or
/// [`DeferralReason::BelowThreshold`] once the heap's statistics are fresh at
/// `now`.
fn analyze_baseline(statistics: HeapStatistics, now: DateTime<Utc>) -> Result<u64, DeferralReason> {
    match statistics.freshness(now) {
        StatisticsFreshness::Stale => Ok(statistics.modified_since_analyze),
        StatisticsFreshness::Fresh => Err(DeferralReason::BelowThreshold),
    }
}

/// Read the same measure in `branch` after the action and pair it with the
/// baseline.
pub(crate) async fn read_comparison(connection: &mut PgConnection, branch: Branch, baseline: ActionBaseline) -> Result<ActionComparison, MaintenanceError> {
    Ok(match baseline {
        ActionBaseline::Index { index, before_bytes } => ActionComparison::Index {
            before_bytes,
            after_bytes: inspection::index_bytes(connection, branch, index).await?,
        },
        ActionBaseline::Heap { heap, before } => ActionComparison::Heap {
            before,
            after: inspection::heap_tuple_counts(connection, branch, heap).await?,
        },
        ActionBaseline::Statistics { heap, before_modified } => ActionComparison::Statistics {
            before_modified,
            after_modified: statistics_freshness::heap_statistics_of_target(connection, branch, heap)
                .await?
                .modified_since_analyze,
        },
        ActionBaseline::Columnstore { chunk } => ActionComparison::Columnstore {
            after: columnstore_partiality::chunk_partiality(connection, chunk).await?,
        },
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        action_measurement::TupleCounts,
        candidate::heap_target::HeapTarget,
        inspection::HeapSample,
        observed_heap::WatchedHeap,
        policy::{DeferralReason, DegradationSample, PolicyProfile},
        prepared_action::{analyze_baseline, vacuum_baseline},
        statistics_freshness::{AnalyzeProgress, HeapStatistics},
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceProfile;
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;

    const MIB: u64 = 1_048_576;

    /// A 1 GiB heap holding `dead` dead and `live` live tuples, sampled
    /// against the responsive profile's 10% dead-space threshold. At 5% dead
    /// it still holds 51 MiB to reclaim, over the 16 MiB floor, so the dead
    /// share alone decides.
    fn heap_sample(dead: u64, live: u64, profile: PolicyProfile) -> HeapSample {
        let counts = TupleCounts { dead, live };
        HeapSample {
            target: HeapTarget::CurrentAttributes,
            counts,
            degradation: DegradationSample {
                relation_bytes: 1024 * MIB,
                reclaimable_bytes: (1024 * MIB).saturating_mul(dead) / (dead + live),
                degraded_percent: counts.dead_percent().unwrap_or_default(),
                threshold_percent: profile.heap_dead_percent,
            },
        }
    }

    fn statistics(modified_since_analyze: u64, last_analyzed: Option<DateTime<Utc>>) -> HeapStatistics {
        HeapStatistics {
            heap: WatchedHeap::Target(HeapTarget::CurrentAttributes),
            modified_since_analyze,
            analyze_trigger: 20_500,
            last_analyzed,
            analyze: AnalyzeProgress::Idle,
        }
    }

    /// 30,000 dead of 100,000 tuples is 30%; once autovacuum leaves 5,000 of
    /// 100,000 it is 5%, under the 10% that queued the vacuum.
    #[test]
    fn a_queued_vacuum_whose_dead_share_fell_under_the_threshold_lapses() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        assert_eq!(
            vacuum_baseline(heap_sample(30_000, 70_000, profile), profile),
            Ok(TupleCounts { dead: 30_000, live: 70_000 })
        );
        assert_eq!(
            vacuum_baseline(heap_sample(5_000, 95_000, profile), profile),
            Err(DeferralReason::BelowThreshold)
        );
    }

    /// Half the tuples are dead, but the heap now measures 10 MiB, under the
    /// responsive profile's 16 MiB floor.
    #[test]
    fn a_queued_vacuum_of_a_heap_shrunk_under_the_size_floor_lapses() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let mut sample = heap_sample(5_000, 5_000, profile);
        sample.degradation.relation_bytes = 10 * MIB;
        assert_eq!(vacuum_baseline(sample, profile), Err(DeferralReason::RelationTooSmall));
    }

    /// Twice the 20,500 trigger is 41,000 modified tuples. After autoanalyze
    /// caught up, 116 remain.
    #[test]
    fn a_queued_analyze_whose_modified_tuples_fell_under_twice_the_trigger_lapses() {
        let now = Utc::now();
        assert_eq!(analyze_baseline(statistics(41_000, None), now), Ok(41_000));
        assert_eq!(analyze_baseline(statistics(116, None), now), Err(DeferralReason::BelowThreshold));
    }

    #[test]
    fn a_queued_analyze_of_a_heap_analyzed_within_ten_minutes_lapses() {
        let now = Utc::now();
        assert_eq!(
            analyze_baseline(statistics(50_000, Some(now - Duration::minutes(1))), now),
            Err(DeferralReason::BelowThreshold)
        );
    }

    #[test]
    fn a_queued_analyze_of_a_heap_being_analyzed_lapses() {
        let running = HeapStatistics {
            analyze: AnalyzeProgress::Running,
            ..statistics(50_000, None)
        };
        assert_eq!(analyze_baseline(running, Utc::now()), Err(DeferralReason::BelowThreshold));
    }
}
