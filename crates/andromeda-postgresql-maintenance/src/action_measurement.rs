//! What one maintenance action is measured by, read immediately before and
//! after it runs, and the effect the two readings show.
//!
//! A rebuild writes a new index file, so physical bytes measure it. A vacuum
//! runs without truncation and never shrinks its heap's file, so bytes would
//! report nothing for every vacuum; its dead-tuple count measures it instead.
//! An analyze is measured by the tuples modified since the heap's last
//! analyze, which it resets; it reclaims nothing.

use crate::{
    candidate::{heap_target::HeapTarget, index_target::IndexTarget},
    catalog_counter::percent,
};
use andromeda_telemetry::maintenance::ActionEffect;

/// Dead and live tuple counts of one heap, from its cumulative statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TupleCounts {
    /// `n_dead_tup`: dead tuples, after a vacuum those it could not remove.
    pub(crate) dead: u64,
    /// `n_live_tup`: live tuples.
    pub(crate) live: u64,
}

impl TupleCounts {
    /// Dead tuples as a whole percentage of all tuples, `None` for a heap
    /// holding none.
    pub(crate) fn dead_percent(self) -> Option<u8> {
        let tuples = self.dead.saturating_add(self.live);
        (tuples != 0).then(|| percent(self.dead, tuples))
    }
}

/// The reading taken before an action, naming the target it belongs to so the
/// reading after is of the same kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionBaseline {
    /// An index about to be rebuilt, with its physical bytes.
    Index {
        /// The index.
        index: IndexTarget,
        /// Physical bytes before the rebuild.
        before_bytes: u64,
    },
    /// A heap about to be vacuumed, with its tuple counts.
    Heap {
        /// The heap.
        heap: HeapTarget,
        /// Tuple counts before the vacuum.
        before: TupleCounts,
    },
    /// A heap about to be analyzed, with its modified tuples.
    Statistics {
        /// The heap.
        heap: HeapTarget,
        /// Tuples modified since the last analyze, before this one.
        before_modified: u64,
    },
}

/// One action's target read immediately before and after it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionComparison {
    /// An index rebuild, compared by physical bytes.
    Index {
        /// Physical bytes before the rebuild.
        before_bytes: u64,
        /// Physical bytes after the rebuild.
        after_bytes: u64,
    },
    /// A heap vacuum, compared by tuple counts.
    Heap {
        /// Tuple counts before the vacuum.
        before: TupleCounts,
        /// Tuple counts after the vacuum.
        after: TupleCounts,
    },
    /// A heap analyze, compared by the tuples modified since the last analyze.
    Statistics {
        /// Modified tuples before the analyze.
        before_modified: u64,
        /// Modified tuples after the analyze, those written while it ran.
        after_modified: u64,
    },
}

impl ActionComparison {
    /// What the action achieved, `None` for an analyze, which reclaims
    /// nothing. A counter that rose, because writes outran the action, reports
    /// zero.
    pub(crate) const fn effect(self) -> Option<ActionEffect> {
        match self {
            ActionComparison::Index { before_bytes, after_bytes } => Some(ActionEffect::ReclaimedBytes(before_bytes.saturating_sub(after_bytes))),
            ActionComparison::Heap { before, after } => Some(ActionEffect::RemovedDeadTuples(before.dead.saturating_sub(after.dead))),
            ActionComparison::Statistics { .. } => None,
        }
    }

    /// The dead share a vacuum left behind, `None` for a rebuild, an analyze
    /// and an empty heap.
    pub(crate) fn post_vacuum_dead_percent(self) -> Option<u8> {
        match self {
            ActionComparison::Index { .. } | ActionComparison::Statistics { .. } => None,
            ActionComparison::Heap { after, .. } => after.dead_percent(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::action_measurement::{ActionComparison, TupleCounts};
    use andromeda_telemetry::maintenance::ActionEffect;
    use pretty_assertions::assert_eq;

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn heap_action_effect_is_removed_dead_tuples() {
        let comparison = ActionComparison::Heap {
            before: TupleCounts { dead: 50_000, live: 100_000 },
            after: TupleCounts { dead: 1_000, live: 100_000 },
        };
        assert_eq!(comparison.effect(), Some(ActionEffect::RemovedDeadTuples(49_000)));
    }

    #[test]
    fn index_action_effect_is_reclaimed_bytes() {
        let comparison = ActionComparison::Index {
            before_bytes: 100 * MIB,
            after_bytes: 40 * MIB,
        };
        assert_eq!(comparison.effect(), Some(ActionEffect::ReclaimedBytes(62_914_560)));
        assert_eq!(comparison.post_vacuum_dead_percent(), None);
    }

    #[test]
    fn dead_tuple_counters_that_rise_report_zero_removed() {
        let comparison = ActionComparison::Heap {
            before: TupleCounts { dead: 10, live: 100 },
            after: TupleCounts { dead: 20, live: 100 },
        };
        assert_eq!(comparison.effect(), Some(ActionEffect::RemovedDeadTuples(0)));
    }

    #[test]
    fn an_analyze_has_no_reclaim_effect() {
        let comparison = ActionComparison::Statistics {
            before_modified: 50_000,
            after_modified: 12,
        };
        assert_eq!(comparison.effect(), None);
        assert_eq!(comparison.post_vacuum_dead_percent(), None);
    }

    #[test]
    fn post_vacuum_dead_share_is_dead_over_all_tuples() {
        let vacuumed = ActionComparison::Heap {
            before: TupleCounts { dead: 500, live: 80 },
            after: TupleCounts { dead: 20, live: 80 },
        };
        assert_eq!(vacuumed.post_vacuum_dead_percent(), Some(20));
        let emptied = ActionComparison::Heap {
            before: TupleCounts { dead: 500, live: 0 },
            after: TupleCounts { dead: 0, live: 0 },
        };
        assert_eq!(emptied.post_vacuum_dead_percent(), None);
    }
}
