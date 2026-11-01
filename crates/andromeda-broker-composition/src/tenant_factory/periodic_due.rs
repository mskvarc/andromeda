//! When one tenant's periodic notifications next fall due, as its periodic
//! sweeps last reported it.
//!
//! The runtime cache reads it whenever it makes room for another tenant, on the
//! request path, so each sweep's instant is held in an atomic and read without
//! a database round trip.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// One of the periodic sweeps a tenant runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PeriodicSweep {
    /// The sweep sending clause 5.8.6 notifications of `timeInterval`
    /// Subscriptions.
    Entity,
    /// The sweep sending clause 5.11.7 notifications of `timeInterval`
    /// context source Subscriptions.
    ContextSource,
}

/// The instant each of a tenant's periodic sweeps reported it is next due.
#[derive(Debug, Default)]
pub(super) struct PeriodicDue {
    /// What the entity sweep last reported.
    entity: DueSlot,
    /// What the context source sweep last reported.
    context_source: DueSlot,
}

impl PeriodicDue {
    /// Record what `sweep` reported after a pass: the instant it is next due,
    /// or `None` when it has nothing scheduled.
    pub(super) fn publish(&self, sweep: PeriodicSweep, due: Option<SystemTime>) {
        match sweep {
            PeriodicSweep::Entity => self.entity.store(due),
            PeriodicSweep::ContextSource => self.context_source.store(due),
        }
    }

    /// The earliest instant any sweep is next due, or `None` when none has
    /// anything scheduled.
    pub(super) fn earliest(&self) -> Option<SystemTime> {
        self.entity.load().into_iter().chain(self.context_source.load()).min()
    }
}

/// One sweep's next due instant, in milliseconds since the Unix epoch plus one.
///
/// Zero means nothing is scheduled, so the slot is one atomic word. An instant
/// before the epoch is held as the epoch: it is already past, as due as can be.
#[derive(Debug, Default)]
struct DueSlot(AtomicU64);

impl DueSlot {
    /// Hold `due`.
    fn store(&self, due: Option<SystemTime>) {
        let stamp = due.map_or(0, |due| {
            let since_epoch = due.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
            u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX - 1).saturating_add(1)
        });
        self.0.store(stamp, Ordering::Release);
    }

    /// The instant held, to the millisecond.
    fn load(&self) -> Option<SystemTime> {
        match self.0.load(Ordering::Acquire) {
            0 => None,
            stamp => Some(UNIX_EPOCH + Duration::from_millis(stamp - 1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::periodic_due::{PeriodicDue, PeriodicSweep};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn at(millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(millis)
    }

    #[test]
    fn nothing_is_due_before_any_sweep_reports() {
        pretty_assertions::assert_eq!(PeriodicDue::default().earliest(), None);
    }

    #[test]
    fn the_earliest_of_the_two_sweeps_is_due() {
        let due = PeriodicDue::default();
        due.publish(PeriodicSweep::Entity, Some(at(5_000)));
        due.publish(PeriodicSweep::ContextSource, Some(at(2_000)));
        pretty_assertions::assert_eq!(due.earliest(), Some(at(2_000)));
    }

    #[test]
    fn a_sweep_reporting_nothing_scheduled_clears_only_its_own_instant() {
        let due = PeriodicDue::default();
        due.publish(PeriodicSweep::Entity, Some(at(5_000)));
        due.publish(PeriodicSweep::ContextSource, Some(at(2_000)));
        due.publish(PeriodicSweep::ContextSource, None);
        pretty_assertions::assert_eq!(due.earliest(), Some(at(5_000)));
        due.publish(PeriodicSweep::Entity, None);
        pretty_assertions::assert_eq!(due.earliest(), None);
    }

    #[test]
    fn the_epoch_itself_is_held_as_an_instant_not_as_nothing_scheduled() {
        let due = PeriodicDue::default();
        due.publish(PeriodicSweep::Entity, Some(UNIX_EPOCH));
        pretty_assertions::assert_eq!(due.earliest(), Some(UNIX_EPOCH));
    }

    #[test]
    fn an_instant_is_held_to_the_millisecond() {
        let due = PeriodicDue::default();
        due.publish(PeriodicSweep::Entity, Some(UNIX_EPOCH + Duration::from_micros(1_234_567)));
        pretty_assertions::assert_eq!(due.earliest(), Some(at(1_234)));
    }
}
