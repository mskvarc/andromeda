//! The tenant wakes that are overdue, and how long the oldest has waited.
//!
//! A wake is overdue when its tenant's work is due and the pass that should
//! have loaded the tenant did not. Either the pass claimed its whole budget and
//! left the wake unclaimed in the schedule, or it claimed the wake and deferred
//! it because no loaded tenant could be unloaded. The schedule holds the
//! instant an unclaimed wake fell due. A deferred wake is rescheduled for its
//! retry, so the schedule no longer shows it due: this process keeps it, with
//! the instant its work fell due, until the tenant is loaded or the retry falls
//! due and the schedule shows the wake again.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use dashmap::DashMap;
use std::{
    num::NonZeroUsize,
    time::{Duration, SystemTime},
};

/// A number of overdue wakes and the instant the oldest of them fell due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverdueWakes {
    /// How many wakes are overdue.
    count: NonZeroUsize,
    /// The instant the oldest of them fell due.
    oldest_due: SystemTime,
}

impl OverdueWakes {
    /// `count` overdue wakes, the oldest of which fell due at `oldest_due`.
    #[must_use]
    pub const fn new(count: NonZeroUsize, oldest_due: SystemTime) -> OverdueWakes {
        OverdueWakes { count, oldest_due }
    }

    /// One overdue wake, due since `due`.
    const fn one(due: SystemTime) -> OverdueWakes {
        OverdueWakes::new(NonZeroUsize::MIN, due)
    }

    /// How many wakes are overdue.
    #[must_use]
    pub const fn count(&self) -> NonZeroUsize {
        self.count
    }

    /// How long the oldest has been due at `now`; zero when it fell due after
    /// `now`.
    #[must_use]
    pub fn oldest_age_at(&self, now: SystemTime) -> Duration {
        now.duration_since(self.oldest_due).unwrap_or_default()
    }

    /// These wakes and `other` together.
    #[must_use]
    pub fn and(self, other: OverdueWakes) -> OverdueWakes {
        OverdueWakes::new(self.count.saturating_add(other.count.get()), self.oldest_due.min(other.oldest_due))
    }
}

/// A wake this process deferred.
#[derive(Debug, Clone, Copy)]
struct DeferredWake {
    /// The instant the tenant's work fell due.
    due: SystemTime,
    /// The instant the schedule wakes the tenant again.
    retry_at: SystemTime,
}

/// The wakes this process deferred and has not since served, until their
/// retry falls due.
#[derive(Debug, Default)]
pub(crate) struct WakeBacklog {
    /// Each deferred tenant's wake.
    deferred: DashMap<TenantName, DeferredWake>,
}

impl WakeBacklog {
    /// Note that `tenant`'s wake, due since `due`, was deferred to `retry_at`.
    /// A tenant deferred again keeps the earlier of its due instants.
    pub(crate) fn deferred(&self, tenant: &TenantName, due: SystemTime, retry_at: SystemTime) {
        self.deferred
            .entry(tenant.clone())
            .and_modify(|wake| {
                wake.due = wake.due.min(due);
                wake.retry_at = retry_at;
            })
            .or_insert(DeferredWake { due, retry_at });
    }

    /// Note that `tenant`'s wake no longer waits: it was loaded, found already
    /// loaded, or found with nothing due.
    pub(crate) fn served(&self, tenant: &TenantName) {
        drop(self.deferred.remove(tenant));
    }

    /// The deferred wakes whose retry is still ahead of `now`. A wake whose
    /// retry fell due is dropped: the schedule shows it due again, so it is
    /// counted there.
    pub(crate) fn pending_at(&self, now: SystemTime) -> Option<OverdueWakes> {
        self.deferred.retain(|_, wake| wake.retry_at > now);
        self.deferred.iter().map(|wake| OverdueWakes::one(wake.due)).reduce(OverdueWakes::and)
    }
}

#[cfg(test)]
mod tests {
    use crate::wake_backlog::{OverdueWakes, WakeBacklog};
    use andromeda_ngsi_ld_lexicon::tenant::{InvalidTenantName, TenantName};
    use pretty_assertions::assert_eq;
    use std::{
        num::NonZeroUsize,
        time::{Duration, SystemTime},
    };

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn tenant(name: &str) -> Result<TenantName, InvalidTenantName> {
        TenantName::try_from(name.to_owned())
    }

    fn overdue(count: usize, oldest_due: SystemTime) -> Option<OverdueWakes> {
        NonZeroUsize::new(count).map(|count| OverdueWakes::new(count, oldest_due))
    }

    #[test]
    fn deferred_wakes_are_counted_with_the_earliest_instant_their_work_fell_due() -> Result<(), InvalidTenantName> {
        let backlog = WakeBacklog::default();
        backlog.deferred(&tenant("first")?, at(40), at(110));
        backlog.deferred(&tenant("second")?, at(25), at(120));
        backlog.deferred(&tenant("third")?, at(70), at(115));

        let pending = backlog.pending_at(at(100));

        assert_eq!(pending, overdue(3, at(25)));
        assert_eq!(pending.map(|wakes| wakes.oldest_age_at(at(100))), Some(Duration::from_secs(75)));
        Ok(())
    }

    #[test]
    fn a_tenant_deferred_again_is_counted_once_from_its_first_due_instant() -> Result<(), InvalidTenantName> {
        let backlog = WakeBacklog::default();
        backlog.deferred(&tenant("periodic")?, at(10), at(20));
        backlog.deferred(&tenant("periodic")?, at(18), at(30));

        assert_eq!(backlog.pending_at(at(25)), overdue(1, at(10)));
        Ok(())
    }

    #[test]
    fn a_served_wake_is_no_longer_overdue() -> Result<(), InvalidTenantName> {
        let backlog = WakeBacklog::default();
        backlog.deferred(&tenant("served")?, at(10), at(50));
        backlog.deferred(&tenant("waiting")?, at(30), at(50));

        backlog.served(&tenant("served")?);

        assert_eq!(backlog.pending_at(at(40)), overdue(1, at(30)));
        Ok(())
    }

    #[test]
    fn a_wake_whose_retry_fell_due_is_left_to_the_schedule() -> Result<(), InvalidTenantName> {
        let backlog = WakeBacklog::default();
        backlog.deferred(&tenant("retried")?, at(10), at(40));

        assert_eq!(backlog.pending_at(at(39)), overdue(1, at(10)));
        assert_eq!(backlog.pending_at(at(40)), None);
        assert_eq!(backlog.pending_at(at(30)), None, "a dropped wake is not counted again");
        Ok(())
    }

    #[test]
    fn overdue_wakes_together_add_their_counts_and_keep_the_older_instant() {
        let deferred = overdue(1, at(50));
        let unclaimed = overdue(4, at(30));

        let both = deferred.zip(unclaimed).map(|(deferred, unclaimed)| deferred.and(unclaimed));

        assert_eq!(both, overdue(5, at(30)));
        assert_eq!(
            both.map(|wakes| wakes.oldest_age_at(at(20))),
            Some(Duration::ZERO),
            "a wake due after the instant has waited for nothing"
        );
    }
}
