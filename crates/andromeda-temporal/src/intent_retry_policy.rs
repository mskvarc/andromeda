//! How a rejected temporal append intent is rescheduled and when it is
//! reported as stuck.

use crate::{error::TemporalError, temporal_invariant::TemporalInvariant};
use andromeda_context_commit::failure_summary::{FAILURE_SUMMARY_LIMIT, bounded_failure_summary};
use andromeda_retry::{retry_number::RetryNumber, schedule::BackoffSchedule};
use chrono::{DateTime, TimeDelta, Utc};
use std::{fmt::Display, num::NonZeroU32};

/// The retry schedule and stuck threshold of the temporal intent drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalIntentRetryPolicy {
    /// The delay before each retry, read from the completed-attempt count.
    schedule: BackoffSchedule,
    /// Completed-attempt count from which an intent is reported as stuck.
    stuck_after: NonZeroU32,
}

impl TemporalIntentRetryPolicy {
    /// A policy retrying on `schedule` and reporting an intent as stuck from
    /// `stuck_after` completed attempts.
    ///
    /// The schedule is expected to be bounded: there is no dead-letter for an
    /// intent, and a bounded cadence is what lets a repaired sink drain the
    /// backlog promptly.
    #[must_use]
    pub const fn new(schedule: BackoffSchedule, stuck_after: NonZeroU32) -> TemporalIntentRetryPolicy {
        TemporalIntentRetryPolicy { schedule, stuck_after }
    }

    /// The next eligible claim time after `attempts` completed attempts.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalInvariant::RetryDelayOutOfRange`] when the delay cannot be expressed
    /// as a calendar duration.
    pub fn retry_at(&self, now: DateTime<Utc>, attempts: u32) -> Result<DateTime<Utc>, TemporalError> {
        let delay = self.schedule.delay(RetryNumber::new(NonZeroU32::MIN.saturating_add(attempts)));
        let delay = TimeDelta::from_std(delay).map_err(|error| TemporalError::from(TemporalInvariant::RetryDelayOutOfRange(error)))?;
        Ok(now + delay)
    }

    /// Whether an intent that has now failed `attempts` completed attempts is
    /// to be reported as stuck.
    #[must_use]
    pub const fn is_stuck(&self, attempts: u32) -> bool {
        attempts >= self.stuck_after.get()
    }

    /// The failure summary the queue row keeps, cut to the bound the
    /// `last_error` check of `temporal_append_intents` states so the retry never
    /// fails on its own bookkeeping.
    pub fn summary(error: &(impl Display + ?Sized)) -> String {
        bounded_failure_summary(&error.to_string(), FAILURE_SUMMARY_LIMIT).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use crate::intent_retry_policy::TemporalIntentRetryPolicy;
    use andromeda_context_commit::failure_summary::FAILURE_SUMMARY_MAX_CHARACTERS;
    use andromeda_retry::{growth::Growth, schedule::BackoffSchedule};
    use chrono::{TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    fn policy() -> TemporalIntentRetryPolicy {
        TemporalIntentRetryPolicy::new(
            BackoffSchedule::new(Duration::from_secs(2), Duration::from_secs(5), Growth::Doubling),
            NonZeroU32::new(3).expect("non-zero"),
        )
    }

    #[test]
    fn the_delay_grows_with_the_attempt_count_until_the_ceiling() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let policy = policy();
        assert_eq!(policy.retry_at(now, 0).expect("in range"), now + TimeDelta::seconds(2));
        assert_eq!(policy.retry_at(now, 1).expect("in range"), now + TimeDelta::seconds(4));
        assert_eq!(policy.retry_at(now, 2).expect("in range"), now + TimeDelta::seconds(5));
        assert_eq!(policy.retry_at(now, u32::MAX).expect("in range"), now + TimeDelta::seconds(5));
    }

    #[test]
    fn an_intent_is_stuck_from_the_configured_attempt_on() {
        let policy = policy();
        assert!(!policy.is_stuck(2));
        assert!(policy.is_stuck(3));
        assert!(policy.is_stuck(4));
    }

    #[test]
    fn a_long_summary_is_cut_at_the_column_limit_on_a_character_boundary() {
        let long = "é".repeat(3000);
        let summary = TemporalIntentRetryPolicy::summary(&long);
        assert_eq!(summary.chars().count(), usize::try_from(FAILURE_SUMMARY_MAX_CHARACTERS).expect("fits usize"));
        assert!(summary.chars().all(|character| character == 'é'));
        assert_eq!(TemporalIntentRetryPolicy::summary("boom"), "boom");
    }
}
