//! The order in which unleased runtimes are unloaded to make room.
//!
//! Last use alone would make a tenant whose periodic notification falls due in
//! a second as good a victim as one with nothing due. Unloaded, that tenant is
//! woken no earlier than `min_wake_interval` later, so its notification misses
//! the instant clause 5.8.6 sends it at. Such a runtime is therefore unloaded
//! only after every runtime without imminent periodic work, and among runtimes
//! of one kind the least recently used goes first.

use std::time::{Instant, SystemTime};

/// Whether a runtime's next periodic notification falls due before it could be
/// woken again once unloaded.
///
/// Declared in eviction order: a runtime with distant work is unloaded before
/// one with imminent work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PeriodicWork {
    /// Nothing periodic is scheduled, or the next notification falls due at or
    /// after the horizon.
    Distant,
    /// The next periodic notification falls due before the horizon.
    Imminent,
}

impl PeriodicWork {
    /// Classify `next_due` against `horizon`, the instant `min_wake_interval`
    /// from now before which an unloaded tenant is not woken again.
    pub(super) fn of(next_due: Option<SystemTime>, horizon: SystemTime) -> PeriodicWork {
        match next_due {
            Some(due) if due < horizon => PeriodicWork::Imminent,
            Some(_) | None => PeriodicWork::Distant,
        }
    }
}

/// Whether a runtime holds changes it has accepted and not yet acted on.
///
/// Declared in eviction order: a runtime with nothing held is unloaded before
/// one with a backlog, which would act on the backlog under a bounded drain
/// while the tenant's own workers stop, and can lose what the drain cannot
/// finish in time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Backlog {
    /// The runtime holds no change it has yet to act on.
    Clear,
    /// The runtime holds at least one change it has yet to act on.
    Held,
}

impl Backlog {
    /// Classify the number of changes a runtime holds.
    pub(super) const fn of(undelivered: usize) -> Backlog {
        if undelivered == 0 { Backlog::Clear } else { Backlog::Held }
    }
}

/// Where one runtime stands in the eviction order: lower ranks are unloaded
/// first.
///
/// Field order is the comparison order the derived `Ord` follows: whether a
/// backlog is held first, then the kind of periodic work, then last use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EvictionRank {
    /// Whether the runtime holds changes it has yet to act on.
    pub(super) backlog: Backlog,
    /// Whether the runtime has periodic work due before it could be woken.
    pub(super) work: PeriodicWork,
    /// When the runtime was last used.
    pub(super) last_used: Instant,
}

#[cfg(test)]
mod tests {
    use crate::runtime::eviction_rank::{Backlog, EvictionRank, PeriodicWork};
    use std::time::{Duration, Instant, SystemTime};

    #[test]
    fn a_due_instant_before_the_horizon_is_imminent() {
        let horizon = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        pretty_assertions::assert_eq!(PeriodicWork::of(Some(horizon - Duration::from_millis(1)), horizon), PeriodicWork::Imminent);
    }

    #[test]
    fn a_due_instant_at_or_after_the_horizon_or_none_is_distant() {
        let horizon = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        pretty_assertions::assert_eq!(PeriodicWork::of(Some(horizon), horizon), PeriodicWork::Distant);
        pretty_assertions::assert_eq!(PeriodicWork::of(Some(horizon + Duration::from_secs(1)), horizon), PeriodicWork::Distant);
        pretty_assertions::assert_eq!(PeriodicWork::of(None, horizon), PeriodicWork::Distant);
    }

    #[test]
    fn distant_work_ranks_before_imminent_work_however_recently_used() {
        let earlier = Instant::now();
        let later = earlier + Duration::from_secs(60);
        let imminent_and_cold = EvictionRank {
            backlog: Backlog::Clear,
            work: PeriodicWork::Imminent,
            last_used: earlier,
        };
        let distant_and_warm = EvictionRank {
            backlog: Backlog::Clear,
            work: PeriodicWork::Distant,
            last_used: later,
        };
        assert!(distant_and_warm < imminent_and_cold);
    }

    #[test]
    fn runtimes_of_one_kind_rank_by_last_use() {
        let earlier = Instant::now();
        let later = earlier + Duration::from_secs(60);
        for work in [PeriodicWork::Distant, PeriodicWork::Imminent] {
            assert!(
                EvictionRank {
                    backlog: Backlog::Clear,
                    work,
                    last_used: earlier
                } < EvictionRank {
                    backlog: Backlog::Clear,
                    work,
                    last_used: later
                }
            );
        }
    }

    #[test]
    fn a_runtime_holding_changes_ranks_after_one_holding_none_whatever_else_it_has() {
        let earlier = Instant::now();
        let later = earlier + Duration::from_secs(60);
        let held_and_cold = EvictionRank {
            backlog: Backlog::of(3),
            work: PeriodicWork::Distant,
            last_used: earlier,
        };
        let clear_and_imminent = EvictionRank {
            backlog: Backlog::of(0),
            work: PeriodicWork::Imminent,
            last_used: later,
        };
        assert!(clear_and_imminent < held_and_cold);
    }
}
