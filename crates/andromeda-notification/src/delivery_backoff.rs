//! How long a failed notification delivery waits before it is due again.
//!
//! The queue persists how many attempts a delivery has failed, so the wait is
//! read from that count rather than from a streak held in memory: the failure
//! being acknowledged is the retry numbered one past the persisted count.
//!
//! A wait is a step, not an instant. The queue adds it to its own clock when
//! it records the failure, the clock every claim compares a due instant with,
//! so a broker whose clock differs from the store's neither delays a retry nor
//! hastens it. Every step is therefore one the store can add to any instant it
//! holds: a schedule growing past [`DeliveryBackoff::MAX_STEP`] is refused.

use andromeda_configuration::human_duration::HumanDuration;
use andromeda_retry::{
    retry_number::{RetryNumber, RetryNumberError},
    schedule::BackoffSchedule,
};
use chrono::{DateTime, TimeDelta, Utc};
use std::time::Duration;
use thiserror::Error;

/// A retry schedule whose steps the delivery queue cannot add to its clock.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryBackoffError {
    /// The longest delay of the schedule exceeds [`DeliveryBackoff::MAX_STEP`].
    #[error(
        "a retry delay of up to {} exceeds the longest retry step of {}",
        HumanDuration::new(*ceiling),
        HumanDuration::new(*maximum)
    )]
    StepTooLong {
        /// The longest delay of the refused schedule.
        ceiling: Duration,
        /// The longest accepted step.
        maximum: Duration,
    },
}

/// The schedule a failed notification delivery is retried on, every step of
/// which is at most [`DeliveryBackoff::MAX_STEP`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryBackoff(BackoffSchedule);

impl DeliveryBackoff {
    /// The longest step: one day.
    pub const MAX_STEP: Duration = Duration::from_hours(24);

    /// Retry on `schedule`.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryBackoffError::StepTooLong`] when the longest delay of
    /// `schedule` exceeds [`DeliveryBackoff::MAX_STEP`].
    pub const fn new(schedule: BackoffSchedule) -> Result<DeliveryBackoff, DeliveryBackoffError> {
        if schedule.ceiling_bound().as_nanos() > DeliveryBackoff::MAX_STEP.as_nanos() {
            Err(DeliveryBackoffError::StepTooLong {
                ceiling: schedule.ceiling_bound(),
                maximum: DeliveryBackoff::MAX_STEP,
            })
        } else {
            Ok(DeliveryBackoff(schedule))
        }
    }

    /// The wait after the failure of a delivery that had failed
    /// `attempt_count` attempts before it.
    ///
    /// # Errors
    ///
    /// Returns [`RetryNumberError`] when `attempt_count` is negative, which the
    /// queue never records.
    pub fn step(&self, attempt_count: i32) -> Result<BackoffStep, RetryNumberError> {
        let retry = RetryNumber::try_from(attempt_count.saturating_add(1))?;
        Ok(BackoffStep(self.0.delay(retry)))
    }

    /// The wait after a first failure, which a held Subscription's next probe
    /// also waits once its probe is exhausted.
    #[must_use]
    pub fn first_step(&self) -> BackoffStep {
        BackoffStep(self.0.delay(RetryNumber::FIRST))
    }
}

/// The wait before a failed delivery, or the next probe of a held
/// Subscription, is due again; at most [`DeliveryBackoff::MAX_STEP`], since
/// only a validated [`DeliveryBackoff`] makes one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BackoffStep(Duration);

impl BackoffStep {
    /// The wait.
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }

    /// The wait in seconds, fractional below one.
    #[must_use]
    pub const fn seconds(self) -> f64 {
        self.0.as_secs_f64()
    }
}

/// The instant a delivery that had failed `attempt_count` attempts before the
/// one now failing is due again, counted from `now`.
///
/// A due instant past what the calendar represents is the end of time, which
/// no claim reaches.
///
/// # Errors
///
/// Returns [`RetryNumberError`] when `attempt_count` is negative, which the
/// queue never records.
pub fn retry_due_at(backoff: &DeliveryBackoff, attempt_count: i32, now: DateTime<Utc>) -> Result<DateTime<Utc>, RetryNumberError> {
    let step = backoff.step(attempt_count)?;
    Ok(TimeDelta::from_std(step.duration())
        .ok()
        .and_then(|wait| now.checked_add_signed(wait))
        .unwrap_or(DateTime::<Utc>::MAX_UTC))
}

#[cfg(test)]
mod tests {
    use crate::delivery_backoff::{BackoffStep, DeliveryBackoff, DeliveryBackoffError, retry_due_at};
    use andromeda_retry::{growth::Growth, retry_number::RetryNumber, schedule::BackoffSchedule};
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    /// Five seconds doubling, for a queue that allows five attempts.
    fn backoff() -> DeliveryBackoff {
        DeliveryBackoff::new(BackoffSchedule::growing_until(
            Duration::from_secs(5),
            Growth::Doubling,
            RetryNumber::new(NonZeroU32::MIN.saturating_add(5)),
        ))
        .expect("a schedule within the longest step")
    }

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant")
    }

    #[test]
    fn the_first_failure_waits_exactly_the_base_backoff() {
        assert_eq!(backoff().step(0).map(BackoffStep::duration), Ok(Duration::from_secs(5)));
        assert_eq!(backoff().first_step().duration(), Duration::from_secs(5));
    }

    #[test]
    fn each_further_failure_doubles_the_wait_until_the_retry_limit() {
        let waits: Vec<_> = [1, 2, 3, 5, 10]
            .into_iter()
            .map(|attempts| backoff().step(attempts).map(|step| step.duration().as_secs()))
            .collect();
        assert_eq!(waits, [Ok(10), Ok(20), Ok(40), Ok(160), Ok(160)]);
    }

    #[test]
    fn a_negative_attempt_count_is_refused() {
        assert!(backoff().step(-2).is_err());
    }

    #[test]
    fn a_step_reads_as_fractional_seconds() {
        let backoff = DeliveryBackoff::new(BackoffSchedule::constant(Duration::from_millis(100))).expect("a schedule within the longest step");
        assert!((backoff.first_step().seconds() - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn a_schedule_growing_to_the_longest_step_is_accepted_and_one_past_it_refused() {
        let longest = BackoffSchedule::new(Duration::from_secs(5), Duration::from_hours(24), Growth::Doubling);
        let past = BackoffSchedule::new(Duration::from_secs(5), Duration::from_hours(24) + Duration::from_nanos(1), Growth::Doubling);
        assert!(DeliveryBackoff::new(longest).is_ok());
        assert_eq!(
            DeliveryBackoff::new(past),
            Err(DeliveryBackoffError::StepTooLong {
                ceiling: Duration::from_hours(24) + Duration::from_nanos(1),
                maximum: Duration::from_hours(24),
            })
        );
    }

    #[test]
    fn the_due_instant_is_the_step_after_now() {
        assert_eq!(retry_due_at(&backoff(), 1, now()), Ok(now() + TimeDelta::seconds(10)));
    }

    #[test]
    fn a_wait_past_the_calendar_is_due_at_the_end_of_time() {
        assert_eq!(retry_due_at(&backoff(), 0, DateTime::<Utc>::MAX_UTC), Ok(DateTime::<Utc>::MAX_UTC));
    }
}
