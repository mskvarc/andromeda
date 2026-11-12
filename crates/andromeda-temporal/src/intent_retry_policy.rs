//! How a rejected temporal append intent is rescheduled and when it is
//! reported as stuck.

use andromeda_context_commit::failure_summary::{FAILURE_SUMMARY_LIMIT, bounded_failure_summary};
use andromeda_retry::{retry_number::RetryNumber, schedule::BackoffSchedule};
use std::{fmt::Display, num::NonZeroU32, time::Duration};

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

    /// The wait before an intent that failed after `attempts` completed
    /// attempts may be claimed again.
    #[must_use]
    pub fn retry_delay(&self, attempts: u32) -> Duration {
        self.schedule.delay(RetryNumber::new(NonZeroU32::MIN.saturating_add(attempts)))
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
        let policy = policy();
        let delays: Vec<Duration> = [0, 1, 2, u32::MAX].into_iter().map(|attempts| policy.retry_delay(attempts)).collect();
        assert_eq!(delays, [2, 4, 5, 5].map(Duration::from_secs));
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
