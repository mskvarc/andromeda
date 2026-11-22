//! How long a failed notification delivery waits before it is due again.
//!
//! The queue persists how many attempts a delivery has failed, so the wait is
//! read from that count rather than from a streak held in memory: the failure
//! being acknowledged is the retry numbered one past the persisted count.
//!
//! A wait is a step, not an instant. The queue adds it to its own clock when
//! it records the failure, the clock every claim compares a due instant with,
//! so a broker whose clock differs from the store's neither delays a retry nor
//! hastens it.
//!
//! A schedule growing past [`DeliveryBackoff::MAX_STEP`] is refused. The store
//! adds far longer steps to its clock, so the bound is policy: the longest
//! step is also the longest a held Subscription waits for its next probe, and
//! a step past a day only delays the notifications of an endpoint that has
//! recovered.

use andromeda_configuration::human_duration::HumanDuration;
use andromeda_retry::{
    retry_number::{RetryNumber, RetryNumberError},
    schedule::BackoffSchedule,
};
use std::time::Duration;
use thiserror::Error;

/// A retry schedule growing past the longest step a failed delivery waits.
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

    /// The wait after the failure of a delivery that had failed
    /// `attempt_count` attempts before it, when its endpoint asked for
    /// `requested` (RFC 9110 section 10.2.3 `Retry-After`): the step, or the
    /// request when that is longer, up to the longest delay of the schedule,
    /// so an endpoint never holds its Subscription back longer than the
    /// operator allows.
    ///
    /// # Errors
    ///
    /// Returns [`RetryNumberError`] when `attempt_count` is negative, which the
    /// queue never records.
    pub fn step_honouring(&self, attempt_count: i32, requested: Option<Duration>) -> Result<BackoffStep, RetryNumberError> {
        let step = self.step(attempt_count)?;
        Ok(requested.map_or(step, |requested| BackoffStep(step.0.max(requested.min(self.0.ceiling_bound())))))
    }

    /// The wait after a first failure, which a held Subscription's next probe
    /// also waits once its probe is exhausted.
    #[must_use]
    pub fn first_step(&self) -> BackoffStep {
        BackoffStep(self.0.delay(RetryNumber::FIRST))
    }

    /// The wait before a deferred delivery is due again, for a window that
    /// passes `window` from now: at least the first step, so a window already
    /// past does not bring the delivery straight back, and at most
    /// [`DeliveryBackoff::MAX_STEP`], after which a longer window defers it
    /// once more.
    #[must_use]
    pub fn deferral(&self, window: Duration) -> BackoffStep {
        BackoffStep(window.max(self.first_step().duration()).min(DeliveryBackoff::MAX_STEP))
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

#[cfg(test)]
mod tests {
    use crate::delivery_backoff::{BackoffStep, DeliveryBackoff, DeliveryBackoffError};
    use andromeda_retry::{growth::Growth, retry_number::RetryNumber, schedule::BackoffSchedule};
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

    /// The schedule's longest delay is 5 s doubled five times, 160 s.
    #[test]
    fn a_requested_wait_longer_than_the_step_is_waited_up_to_the_longest_delay() {
        let waits: Vec<_> = [
            (0, None),
            (0, Some(Duration::from_secs(3))),
            (0, Some(Duration::from_secs(60))),
            (0, Some(Duration::from_hours(1))),
            (3, Some(Duration::from_secs(10))),
            (3, Some(Duration::from_secs(60))),
        ]
        .into_iter()
        .map(|(attempts, requested)| backoff().step_honouring(attempts, requested).map(|step| step.duration().as_secs()))
        .collect();
        assert_eq!(waits, [Ok(5), Ok(5), Ok(60), Ok(160), Ok(40), Ok(60)]);
    }

    #[test]
    fn a_requested_wait_does_not_excuse_a_negative_attempt_count() {
        assert!(backoff().step_honouring(-2, Some(Duration::from_secs(60))).is_err());
    }

    #[test]
    fn a_deferral_waits_out_its_window_within_the_first_and_the_longest_step() {
        let waits: Vec<_> = [Duration::ZERO, Duration::from_secs(3), Duration::from_secs(60), Duration::from_hours(48)]
            .into_iter()
            .map(|window| backoff().deferral(window).duration())
            .collect();
        assert_eq!(
            waits,
            [
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::from_secs(60),
                Duration::from_hours(24)
            ]
        );
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
}
