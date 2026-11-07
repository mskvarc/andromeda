//! The instant each attempt of one write is stamped at.
//!
//! The store refuses a write stamped earlier than an instant its Entity
//! already holds, and answers that instant ([`WriteOutcome::Predated`]): clause
//! 4.8 makes `modifiedAt` the instant of the last modification, so it never
//! moves backwards, and an instance is never modified before it was created.
//! The next attempt is stamped at the later of the clock and one microsecond
//! past every instant a refusal answered, the microsecond being the precision
//! the store keeps, so it lands after the write that predated it even when the
//! clock stepped back.
//!
//! [`WriteOutcome::Predated`]: crate::write_outcome::WriteOutcome::Predated

use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use chrono::{TimeDelta, Utc};

/// The step past a refused instant: the precision the store keeps.
const ONE_MICROSECOND: TimeDelta = TimeDelta::microseconds(1);

/// The earliest instant the next attempt of one write may be stamped at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttemptInstant {
    /// The latest instant a refusal answered, `None` before any refusal.
    refused_at: Option<SystemInstant>,
}

impl AttemptInstant {
    /// The instant the next attempt is stamped at, the clock read now.
    #[must_use]
    pub fn next(self) -> SystemInstant {
        self.stamp(SystemInstant::new(Utc::now()))
    }

    /// The instant the next attempt is stamped at, given the clock's reading
    /// `clock`: `clock`, or one microsecond past the latest instant a refusal
    /// answered when `clock` is not past it.
    ///
    /// An instant one microsecond short of the last one `chrono` represents
    /// has no successor, and stamps the instant itself, which the store
    /// accepts as it accepts a write at its Entity's latest instant.
    #[must_use]
    pub fn stamp(self, clock: SystemInstant) -> SystemInstant {
        let Some(refused_at) = self.refused_at else {
            return clock;
        };
        let after = refused_at.get().checked_add_signed(ONE_MICROSECOND).map_or(refused_at, SystemInstant::new);
        clock.max(after)
    }

    /// Record that the store refused an attempt because its Entity holds
    /// `latest`.
    pub fn refused(&mut self, latest: SystemInstant) {
        self.refused_at = Some(self.refused_at.map_or(latest, |earlier| earlier.max(latest)));
    }
}

#[cfg(test)]
mod tests {
    use crate::attempt_instant::AttemptInstant;
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use chrono::{DateTime, TimeDelta, Utc};
    use pretty_assertions::assert_eq;

    fn at(text: &str) -> SystemInstant {
        text.parse::<SystemInstant>().expect("a valid instant")
    }

    #[test]
    fn an_attempt_no_refusal_bounds_is_stamped_at_the_clock() {
        assert_eq!(AttemptInstant::default().stamp(at("2026-11-05T10:00:00Z")), at("2026-11-05T10:00:00Z"));
    }

    /// The clock stepped back behind the Entity's latest instant, so the
    /// attempt lands one microsecond after it.
    #[test]
    fn a_clock_behind_the_refused_instant_stamps_one_microsecond_past_it() {
        let mut instant = AttemptInstant::default();
        instant.refused(at("2026-11-05T10:00:05.000007Z"));
        assert_eq!(instant.stamp(at("2026-11-05T10:00:00Z")), at("2026-11-05T10:00:05.000008Z"));
    }

    #[test]
    fn a_clock_at_the_refused_instant_stamps_one_microsecond_past_it() {
        let mut instant = AttemptInstant::default();
        instant.refused(at("2026-11-05T10:00:05Z"));
        assert_eq!(instant.stamp(at("2026-11-05T10:00:05Z")), at("2026-11-05T10:00:05.000001Z"));
    }

    #[test]
    fn a_clock_past_the_refused_instant_stamps_the_clock() {
        let mut instant = AttemptInstant::default();
        instant.refused(at("2026-11-05T10:00:05Z"));
        assert_eq!(instant.stamp(at("2026-11-05T10:00:06Z")), at("2026-11-05T10:00:06Z"));
    }

    /// An attempt refused by a later write is held past it, and a refusal
    /// naming an earlier instant afterwards does not lower that.
    #[test]
    fn the_latest_of_several_refusals_bounds_the_next_attempt() {
        let mut instant = AttemptInstant::default();
        instant.refused(at("2026-11-05T10:00:05Z"));
        instant.refused(at("2026-11-05T10:00:09Z"));
        instant.refused(at("2026-11-05T10:00:07Z"));
        assert_eq!(instant.stamp(at("2026-11-05T10:00:00Z")), at("2026-11-05T10:00:09.000001Z"));
    }

    #[test]
    fn the_last_representable_instant_stamps_itself() {
        let last = SystemInstant::new(DateTime::<Utc>::MAX_UTC);
        let mut instant = AttemptInstant::default();
        instant.refused(last);
        assert_eq!(instant.stamp(at("2026-11-05T10:00:00Z")), last);
        assert!(last.get().checked_add_signed(TimeDelta::microseconds(1)).is_none());
    }
}
