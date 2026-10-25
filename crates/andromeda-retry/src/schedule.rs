//! The delay before any retry, read straight from its retry number.
//!
//! The delay at retry `n` is `jitter(min(first * growth(n), ceiling))`. The
//! arithmetic saturates, so no retry number and no configured duration can
//! overflow or panic, and jitter never lifts a delay past the ceiling.

use crate::{growth::Growth, jitter::Jitter, retry_number::RetryNumber, schedule_error::BackoffScheduleError};
use std::time::Duration;

/// How long to wait before each retry of one kind of operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackoffSchedule {
    /// The un-jittered delay before the first retry.
    first: Duration,
    /// The longest un-jittered delay before any retry.
    ceiling: Duration,
    /// How the delay grows from one retry to the next.
    growth: Growth,
    /// How the delay is spread around its bound.
    jitter: Jitter,
}

impl BackoffSchedule {
    /// A schedule growing from `first` by `growth` until `ceiling`, without
    /// jitter.
    ///
    /// Total for reviewed constants: a ceiling below `first` makes every delay
    /// the ceiling. A schedule read from operator configuration goes through
    /// [`validated`](Self::validated) instead.
    #[must_use]
    pub const fn new(first: Duration, ceiling: Duration, growth: Growth) -> BackoffSchedule {
        BackoffSchedule {
            first,
            ceiling,
            growth,
            jitter: Jitter::None,
        }
    }

    /// The schedule [`new`](Self::new) builds, refused when it could not mean
    /// what its operator wrote.
    ///
    /// # Errors
    ///
    /// [`BackoffScheduleError::ZeroFirst`] when `first` is zero, since a
    /// retry that never waits is a busy loop, and
    /// [`BackoffScheduleError::CeilingBelowFirst`] when `ceiling` is shorter
    /// than `first`.
    pub const fn validated(first: Duration, ceiling: Duration, growth: Growth) -> Result<BackoffSchedule, BackoffScheduleError> {
        if first.is_zero() {
            Err(BackoffScheduleError::ZeroFirst)
        } else if ceiling.as_nanos() < first.as_nanos() {
            Err(BackoffScheduleError::CeilingBelowFirst)
        } else {
            Ok(BackoffSchedule::new(first, ceiling, growth))
        }
    }

    /// The same `delay` before every retry.
    #[must_use]
    pub const fn constant(delay: Duration) -> BackoffSchedule {
        BackoffSchedule::new(delay, delay, Growth::Doubling)
    }

    /// A schedule growing from `first` by `growth` and holding, from retry
    /// `last` onwards, the delay it reaches there.
    #[must_use]
    pub const fn growing_until(first: Duration, growth: Growth, last: RetryNumber) -> BackoffSchedule {
        BackoffSchedule::new(first, growth.grow(first, last), growth)
    }

    /// This schedule with its delays spread by `jitter`.
    #[must_use]
    pub const fn with_jitter(self, jitter: Jitter) -> BackoffSchedule {
        BackoffSchedule { jitter, ..self }
    }

    /// The un-jittered delay before the first retry.
    #[must_use]
    pub const fn first(&self) -> Duration {
        self.first
    }

    /// The longest un-jittered delay before any retry.
    #[must_use]
    pub const fn ceiling_bound(&self) -> Duration {
        self.ceiling
    }

    /// The un-jittered delay before `retry`, the most its jittered delay can
    /// be.
    #[must_use]
    pub fn ceiling(&self, retry: RetryNumber) -> Duration {
        self.growth.grow(self.first, retry).min(self.ceiling)
    }

    /// The delay to wait before `retry`.
    #[must_use]
    pub fn delay(&self, retry: RetryNumber) -> Duration {
        self.jitter.apply(self.ceiling(retry), self.ceiling)
    }
}

#[cfg(test)]
mod tests {
    use crate::{growth::Growth, jitter::Jitter, retry_number::RetryNumber, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    fn retry(ordinal: u32) -> RetryNumber {
        RetryNumber::new(NonZeroU32::new(ordinal).unwrap_or(NonZeroU32::MIN))
    }

    fn delays(schedule: BackoffSchedule, ordinals: impl IntoIterator<Item = u32>) -> Vec<Duration> {
        ordinals.into_iter().map(|ordinal| schedule.delay(retry(ordinal))).collect()
    }

    fn seconds(values: &[u64]) -> Vec<Duration> {
        values.iter().copied().map(Duration::from_secs).collect()
    }

    #[test]
    fn a_doubling_schedule_doubles_until_its_ceiling() {
        let schedule = BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(8), Growth::Doubling);
        assert_eq!(delays(schedule, 1..=6), seconds(&[1, 2, 4, 8, 8, 8]));
    }

    #[test]
    fn the_last_retry_number_holds_the_ceiling_without_overflowing() {
        let schedule = BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(8), Growth::Doubling);
        assert_eq!(schedule.delay(retry(u32::MAX)), Duration::from_secs(8));
    }

    #[test]
    fn a_linear_schedule_grows_by_its_first_delay_until_its_ceiling() {
        let schedule = BackoffSchedule::new(Duration::from_secs(2), Duration::from_secs(5), Growth::Linear);
        assert_eq!(delays(schedule, 1..=4), seconds(&[2, 4, 5, 5]));
    }

    #[test]
    fn a_constant_schedule_waits_the_same_before_every_retry() {
        let schedule = BackoffSchedule::constant(Duration::from_secs(5));
        assert_eq!(delays(schedule, [1, 1000]), seconds(&[5, 5]));
    }

    #[test]
    fn a_ceiling_below_the_first_delay_makes_every_delay_the_ceiling() {
        let schedule = BackoffSchedule::new(Duration::from_secs(10), Duration::from_secs(1), Growth::Doubling);
        assert_eq!(delays(schedule, [1, 2, 50]), seconds(&[1, 1, 1]));
    }

    #[test]
    fn validation_refuses_a_zero_first_delay() {
        assert_eq!(
            BackoffSchedule::validated(Duration::ZERO, Duration::from_secs(1), Growth::Doubling),
            Err(BackoffScheduleError::ZeroFirst)
        );
    }

    #[test]
    fn validation_refuses_a_ceiling_below_the_first_delay() {
        assert_eq!(
            BackoffSchedule::validated(Duration::from_secs(2), Duration::from_secs(1), Growth::Doubling),
            Err(BackoffScheduleError::CeilingBelowFirst)
        );
    }

    #[test]
    fn validation_accepts_a_ceiling_equal_to_the_first_delay() {
        assert_eq!(
            BackoffSchedule::validated(Duration::from_secs(1), Duration::from_secs(1), Growth::Doubling),
            Ok(BackoffSchedule::constant(Duration::from_secs(1)))
        );
    }

    #[test]
    fn growing_until_a_retry_holds_the_delay_reached_there() {
        let schedule = BackoffSchedule::growing_until(Duration::from_secs(5), Growth::Doubling, retry(6));
        assert_eq!(delays(schedule, 1..=8), seconds(&[5, 10, 20, 40, 80, 160, 160, 160]));
    }

    #[test]
    fn growing_until_the_sixteenth_retry_holds_fifteen_doublings() {
        let schedule = BackoffSchedule::growing_until(Duration::from_secs(5), Growth::Doubling, retry(16));
        let held = Duration::from_secs(5 << 15);
        assert_eq!(delays(schedule, [16, 40]), [held, held]);
    }

    #[test]
    fn growing_until_a_retry_past_thirty_two_doublings_stays_exact() {
        let schedule = BackoffSchedule::growing_until(Duration::from_nanos(1), Growth::Doubling, retry(40));
        assert_eq!(schedule.ceiling_bound(), Duration::from_nanos(1 << 39));
        assert_eq!(schedule.delay(retry(33)), Duration::from_nanos(1 << 32));
    }

    #[test]
    fn an_enormous_first_delay_saturates_instead_of_panicking() {
        let schedule = BackoffSchedule::growing_until(Duration::MAX / 2, Growth::Doubling, retry(10));
        assert_eq!(schedule.delay(retry(10)), Duration::MAX);
    }

    #[test]
    fn a_jittered_delay_never_exceeds_the_ceiling_of_its_retry() {
        let schedule = BackoffSchedule::new(Duration::from_millis(2), Duration::from_millis(50), Growth::Doubling).with_jitter(Jitter::Full);
        for ordinal in 1..=12 {
            assert!(schedule.delay(retry(ordinal)) <= schedule.ceiling(retry(ordinal)));
        }
    }

    #[test]
    fn the_accessors_report_the_un_jittered_bounds() {
        let schedule = BackoffSchedule::new(Duration::from_secs(2), Duration::from_secs(30), Growth::Doubling).with_jitter(Jitter::Additive);
        assert_eq!((schedule.first(), schedule.ceiling_bound()), (Duration::from_secs(2), Duration::from_secs(30)));
    }
}
