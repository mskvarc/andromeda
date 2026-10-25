//! Bounded restart and backoff policy.

use crate::failure::FailureClass;
use andromeda_retry::{retry_number::RetryNumber, schedule::BackoffSchedule};
use std::time::Duration;

/// Whether and when a failed task becomes eligible to execute again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart after a failure or unexpected exit.
    Never,
    /// Retry transient failures on a bounded schedule.
    Transient {
        /// Maximum number of restart attempts after the initial attempt.
        max_restarts: u32,
        /// The delay before each restart, read from the consecutive failure
        /// count.
        schedule: BackoffSchedule,
    },
}

impl RestartPolicy {
    /// Construct a transient-only restart policy.
    #[must_use]
    pub const fn transient(max_restarts: u32, schedule: BackoffSchedule) -> RestartPolicy {
        RestartPolicy::Transient { max_restarts, schedule }
    }

    /// Return the next delay for the numbered consecutive failure.
    pub(crate) fn delay(self, class: FailureClass, consecutive_failures: u32) -> Option<Duration> {
        match self {
            RestartPolicy::Transient { max_restarts, schedule } if class == FailureClass::Transient && consecutive_failures <= max_restarts => {
                RetryNumber::try_from(consecutive_failures).ok().map(|retry| schedule.delay(retry))
            }
            RestartPolicy::Never | RestartPolicy::Transient { max_restarts: _, schedule: _ } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{failure::FailureClass, restart_policy::RestartPolicy};
    use andromeda_retry::{growth::Growth, schedule::BackoffSchedule};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn policy() -> RestartPolicy {
        RestartPolicy::transient(4, BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(2), Growth::Doubling))
    }

    #[test]
    fn transient_backoff_is_exponential_and_bounded() {
        let policy = policy();
        assert_eq!(policy.delay(FailureClass::Transient, 1), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay(FailureClass::Transient, 2), Some(Duration::from_secs(2)));
        assert_eq!(policy.delay(FailureClass::Transient, 3), Some(Duration::from_secs(2)));
        assert_eq!(policy.delay(FailureClass::Transient, 5), None);
    }

    #[test]
    fn permanent_and_invariant_failures_never_restart() {
        let policy = policy();
        assert_eq!(policy.delay(FailureClass::Permanent, 1), None);
        assert_eq!(policy.delay(FailureClass::InvariantViolation, 1), None);
    }
}
