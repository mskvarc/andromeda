//! The clock one pass over a leased queue stamps its releases with.

use chrono::{DateTime, TimeDelta, Utc};
use std::time::Instant;

/// The instant one pass over a leased queue started at, advanced by the
/// monotonic time the pass has run since.
///
/// A pass may run for a while, since a delivery attempt is bounded only by its
/// request timeout and a replay by its transaction, so a release it records
/// late in the pass is stamped with the instant it is recorded at, not the
/// instant the pass began. A retry scheduled one backoff after the pass began
/// could otherwise already be due before the pass that scheduled it ends, and
/// be claimed again with no backoff at all. The origin stays the caller's, so
/// a pass driven from an explicit instant keeps that instant as its zero.
#[derive(Clone, Copy, Debug)]
pub struct LeasePassClock {
    /// The instant the pass started at, as its caller gave it.
    origin: DateTime<Utc>,
    /// When the pass started, on the monotonic clock.
    started: Instant,
}

impl LeasePassClock {
    /// Start a pass at `origin`.
    #[must_use]
    pub fn starting_at(origin: DateTime<Utc>) -> LeasePassClock {
        LeasePassClock {
            origin,
            started: Instant::now(),
        }
    }

    /// The instant the pass started at.
    #[must_use]
    pub const fn origin(&self) -> DateTime<Utc> {
        self.origin
    }

    /// The current instant of the pass: its origin advanced by the time it has
    /// run since.
    ///
    /// A pass would have to run for centuries to leave the range a timestamp
    /// holds; if one ever did, the instant saturates at the latest one
    /// representable rather than wrapping back into the past.
    #[must_use]
    pub fn now(&self) -> DateTime<Utc> {
        TimeDelta::from_std(self.started.elapsed())
            .ok()
            .and_then(|elapsed| self.origin.checked_add_signed(elapsed))
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    }
}

#[cfg(test)]
mod tests {
    use crate::lease_pass_clock::LeasePassClock;
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use std::{thread::sleep, time::Duration};

    /// The pass's instant starts at the caller's origin and moves forward with
    /// the time the pass runs, so a release late in the pass is stamped late.
    #[test]
    fn the_pass_instant_advances_from_its_origin_as_the_pass_runs() {
        let origin = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant");
        let clock = LeasePassClock::starting_at(origin);
        let early = clock.now();
        sleep(Duration::from_millis(20));
        let late = clock.now();
        assert!(early >= origin, "{early} precedes the origin {origin}");
        assert!(late >= early + TimeDelta::milliseconds(20), "{late} did not advance past {early}");
        pretty_assertions::assert_eq!(clock.origin(), origin);
    }

    /// An origin at the end of the representable range saturates rather than
    /// wrapping into the past.
    #[test]
    fn a_pass_at_the_end_of_time_saturates() {
        let clock = LeasePassClock::starting_at(DateTime::<Utc>::MAX_UTC);
        sleep(Duration::from_millis(1));
        pretty_assertions::assert_eq!(clock.now(), DateTime::<Utc>::MAX_UTC);
    }
}
