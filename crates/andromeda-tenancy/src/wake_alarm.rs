//! The instant the waker sleeps until, and the signal that ends its sleep early.
//!
//! The waker reads the earliest wake the schedule holds and sleeps until it.
//! A wake this process records after that read, such as the next wake of a
//! tenant it just unloaded, is not in what the waker read. When it is due
//! sooner than the waker's sleep ends, the alarm rings, so the tenant is woken
//! at its own instant and not at the waker's. A wake recorded while a pass
//! runs lowers the instant that pass reports instead, since the waker is not
//! asleep to be woken.
//!
//! A pass that finds no place a woken tenant could take claims nothing and
//! sleeps for room. A place that frees before that sleep ends, as a request
//! lets go of a tenant or a runtime is unloaded, rings the alarm too, so the
//! longest-overdue wake is served the moment room opens.

use std::{
    sync::{
        Mutex,
        MutexGuard,
        PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};
use tokio::sync::Notify;

/// What the waker is doing, as far as a recorded wake is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WakerState {
    /// Running a pass. `recorded` is the earliest wake recorded since it began.
    Passing {
        /// The earliest wake recorded during the pass, if any.
        recorded: Option<SystemTime>,
    },
    /// Asleep until `until`, or until a wake is recorded when `until` is
    /// `None`.
    Sleeping {
        /// The instant the sleep ends without a ring.
        until: Option<SystemTime>,
    },
}

/// The alarm a recorded wake rings when it falls due before the waker's sleep
/// ends.
#[derive(Debug)]
pub struct WakeAlarm {
    /// What the waker is doing.
    state: Mutex<WakerState>,
    /// Set while the waker sleeps for room, cleared by the first ring of a
    /// freed place and by any pass or sleep that is not for room.
    ///
    /// Outside the state lock because every request that lets go of a tenant
    /// reads it: while the waker is not waiting for room, that read is all the
    /// release costs.
    room_awaited: AtomicBool,
    /// Rung once per wake that ends a sleep early. A ring with no one waiting
    /// is kept for the next wait.
    ring: Notify,
}

impl WakeAlarm {
    /// An alarm whose waker sleeps until any wake is recorded.
    #[must_use]
    pub(crate) fn new() -> WakeAlarm {
        WakeAlarm {
            state: Mutex::new(WakerState::Sleeping { until: None }),
            room_awaited: AtomicBool::new(false),
            ring: Notify::new(),
        }
    }

    /// Note that a pass begins: wakes recorded from now on are reported by
    /// [`sleep_until`](Self::sleep_until) rather than rung.
    pub(crate) fn begin_pass(&self) {
        *self.lock() = WakerState::Passing { recorded: None };
        self.room_awaited.store(false, Ordering::Release);
    }

    /// Note that the pass ended and the waker sleeps until `read`, the
    /// earliest wake the schedule held, returning the instant it sleeps until:
    /// `read`, or an earlier wake recorded during the pass.
    pub(crate) fn sleep_until(&self, read: Option<SystemTime>) -> Option<SystemTime> {
        let mut state = self.lock();
        let until = match *state {
            WakerState::Passing { recorded } => earliest(read, recorded),
            WakerState::Sleeping { .. } => read,
        };
        *state = WakerState::Sleeping { until };
        self.room_awaited.store(false, Ordering::Release);
        until
    }

    /// Note that the pass found no room and the waker sleeps until `opens`, the
    /// instant the first woken lifetime ends, or until a place is freed when
    /// `opens` is `None`. Returns the instant it sleeps until, as
    /// [`sleep_until`](Self::sleep_until) does; a freed place rings it sooner.
    pub(crate) fn sleep_for_room(&self, opens: Option<SystemTime>) -> Option<SystemTime> {
        let mut state = self.lock();
        let until = match *state {
            WakerState::Passing { recorded } => earliest(opens, recorded),
            WakerState::Sleeping { .. } => opens,
        };
        *state = WakerState::Sleeping { until };
        self.room_awaited.store(true, Ordering::Release);
        until
    }

    /// Note that a place a woken tenant could take was freed, ringing the
    /// alarm when the waker sleeps for room. Rings once per sleep for room.
    pub(crate) fn room_opened(&self) {
        if self.room_awaited.load(Ordering::Acquire) && self.room_awaited.swap(false, Ordering::AcqRel) {
            self.ring.notify_one();
        }
    }

    /// Note that this process recorded a wake at `at`, ringing the alarm when
    /// the waker sleeps past it.
    pub(crate) fn recorded(&self, at: SystemTime) {
        let mut state = self.lock();
        match *state {
            WakerState::Passing { recorded } => {
                *state = WakerState::Passing {
                    recorded: earliest(recorded, Some(at)),
                }
            }
            WakerState::Sleeping { until } if until.is_none_or(|until| at < until) => {
                *state = WakerState::Sleeping { until: Some(at) };
                self.ring.notify_one();
            }
            WakerState::Sleeping { .. } => {}
        }
    }

    /// Wait for the next ring.
    pub async fn rung(&self) {
        self.ring.notified().await;
    }

    /// The state, whatever a panicking holder left: every write leaves a
    /// whole state, so a poisoned lock still holds one.
    fn lock(&self) -> MutexGuard<'_, WakerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The earlier of two optional instants, either when the other is absent.
fn earliest(first: Option<SystemTime>, second: Option<SystemTime>) -> Option<SystemTime> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::wake_alarm::WakeAlarm;
    use std::time::{Duration, SystemTime};
    use tokio::time::timeout;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    async fn rings(alarm: &WakeAlarm) -> bool {
        timeout(Duration::from_millis(50), alarm.rung()).await.is_ok()
    }

    #[tokio::test]
    async fn a_wake_recorded_before_the_sleep_ends_rings_the_alarm() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        assert_eq!(alarm.sleep_until(Some(at(100))), Some(at(100)));

        alarm.recorded(at(40));

        assert!(rings(&alarm).await);
    }

    #[tokio::test]
    async fn a_wake_recorded_at_or_after_the_sleep_ends_does_not_ring() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.sleep_until(Some(at(100)));

        alarm.recorded(at(100));
        alarm.recorded(at(250));

        assert!(!rings(&alarm).await);
    }

    #[tokio::test]
    async fn only_a_wake_earlier_than_the_last_ring_rings_again() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.sleep_until(Some(at(100)));
        alarm.recorded(at(40));
        assert!(rings(&alarm).await);

        alarm.recorded(at(60));
        assert!(!rings(&alarm).await, "the waker already wakes at 40");

        alarm.recorded(at(30));
        assert!(rings(&alarm).await);
    }

    #[tokio::test]
    async fn any_wake_rings_a_waker_with_nothing_scheduled() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        assert_eq!(alarm.sleep_until(None), None);

        alarm.recorded(at(1_000_000));

        assert!(rings(&alarm).await);
    }

    #[tokio::test]
    async fn a_wake_recorded_during_a_pass_lowers_the_instant_the_pass_sleeps_until() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.recorded(at(70));
        alarm.recorded(at(50));

        assert_eq!(alarm.sleep_until(Some(at(100))), Some(at(50)));
        alarm.begin_pass();
        assert_eq!(alarm.sleep_until(None), None, "a later pass that recorded nothing sleeps until its read");
        assert!(!rings(&alarm).await, "a wake recorded during a pass is reported, not rung");
    }

    #[tokio::test]
    async fn a_wake_recorded_during_a_pass_with_nothing_scheduled_is_slept_until() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.recorded(at(70));

        assert_eq!(alarm.sleep_until(None), Some(at(70)));
    }

    #[tokio::test]
    async fn a_freed_place_rings_a_waker_sleeping_for_room() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        assert_eq!(alarm.sleep_for_room(Some(at(100))), Some(at(100)));

        alarm.room_opened();

        assert!(rings(&alarm).await);
    }

    #[tokio::test]
    async fn a_freed_place_rings_a_waker_sleeping_for_room_once() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.sleep_for_room(None);
        alarm.room_opened();
        assert!(rings(&alarm).await);

        alarm.room_opened();

        assert!(!rings(&alarm).await, "the waker is already woken to use the first freed place");
    }

    #[tokio::test]
    async fn a_freed_place_does_not_ring_a_waker_sleeping_until_a_wake() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.sleep_until(Some(at(100)));

        alarm.room_opened();

        assert!(!rings(&alarm).await);
    }

    #[tokio::test]
    async fn a_freed_place_does_not_ring_during_a_pass() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.sleep_for_room(None);
        alarm.begin_pass();

        alarm.room_opened();

        assert!(!rings(&alarm).await, "the running pass reads the room itself");
    }

    #[tokio::test]
    async fn a_wake_recorded_during_a_pass_without_room_is_slept_until_when_sooner_than_the_room() {
        let alarm = WakeAlarm::new();
        alarm.begin_pass();
        alarm.recorded(at(40));

        assert_eq!(alarm.sleep_for_room(Some(at(100))), Some(at(40)));
    }
}
