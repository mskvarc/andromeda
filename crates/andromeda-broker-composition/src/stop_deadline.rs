//! The one deadline a stopping broker stops by.
//!
//! The HTTP listeners drain within `server.graceful_drain_timeout`, and live
//! notification evaluation within `notifications.evaluation.drain_timeout`.
//! A stop is given the two together, counted from the stop signal: the
//! tenants' teardowns and the process tasks share whatever the HTTP drain
//! leaves, so the grace an orchestrator gives the broker follows from the
//! configuration however many tenants are loaded.

use crate::shutdown_signal::shutdown_signal;
use std::{
    future::Future,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::time::Instant;

/// The longest stop a deadline is set for, a year, whatever drains are
/// configured; it keeps the deadline an instant the clock can hold.
const LONGEST_STOP: Duration = Duration::from_hours(365 * 24);

/// When the broker's stop began: the moment the stop signal arrived.
#[derive(Clone, Debug, Default)]
pub(crate) struct StopClock(Arc<OnceLock<Instant>>);

impl StopClock {
    /// The stop signal, recording the moment it arrives.
    pub(crate) fn signal(&self) -> impl Future<Output = ()> + Send + 'static {
        let arrived = Arc::clone(&self.0);
        async move {
            shutdown_signal().await;
            arrived.get_or_init(Instant::now);
        }
    }

    /// The instant the stop must be over by: its start plus `http_drain` and
    /// `notification_drain`. A stop no signal began, after the listeners
    /// failed, begins now.
    pub(crate) fn deadline(&self, http_drain: Duration, notification_drain: Duration) -> Instant {
        let started = *self.0.get_or_init(Instant::now);
        let allowed = http_drain.saturating_add(notification_drain).min(LONGEST_STOP);
        // A year past the stop is an instant every supported clock holds, so
        // the stop never ends early through this fallback.
        started.checked_add(allowed).unwrap_or(started)
    }
}

#[cfg(test)]
mod tests {
    use crate::stop_deadline::{LONGEST_STOP, StopClock};
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use tokio::time::{Instant, advance};

    #[tokio::test(start_paused = true)]
    async fn a_stop_is_given_both_drains_from_its_start() {
        let clock = StopClock::default();
        let started = Instant::now();
        assert_eq!(
            clock.deadline(Duration::from_secs(30), Duration::from_secs(30)),
            started + Duration::from_secs(60)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_counts_from_the_start_however_late_it_is_read() {
        let clock = StopClock::default();
        let first = clock.deadline(Duration::from_secs(1), Duration::from_secs(2));
        advance(Duration::from_secs(5)).await;
        assert_eq!(clock.deadline(Duration::from_secs(1), Duration::from_secs(2)), first);
    }

    #[tokio::test(start_paused = true)]
    async fn drains_past_a_year_are_given_a_year() {
        let clock = StopClock::default();
        let started = Instant::now();
        assert_eq!(clock.deadline(Duration::MAX, Duration::MAX), started + LONGEST_STOP);
    }
}
