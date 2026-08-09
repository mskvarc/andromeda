//! Bounded restart and exponential-backoff policy.

use crate::failure::FailureClass;
use std::time::Duration;

/// Whether and when a failed task becomes eligible to execute again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart after a failure or unexpected exit.
    Never,
    /// Retry transient failures with bounded exponential backoff.
    Transient {
        /// Maximum number of restart attempts after the initial attempt.
        max_restarts: u32,
        /// Delay before the first restart.
        initial_backoff: Duration,
        /// Maximum delay between attempts.
        maximum_backoff: Duration,
    },
}

impl RestartPolicy {
    /// Construct a transient-only restart policy.
    pub const fn transient(max_restarts: u32, initial_backoff: Duration, maximum_backoff: Duration) -> RestartPolicy {
        RestartPolicy::Transient {
            max_restarts,
            initial_backoff,
            maximum_backoff,
        }
    }

    /// Return the next delay for the numbered consecutive failure.
    pub(crate) fn delay(self, class: FailureClass, consecutive_failures: u32) -> Option<Duration> {
        match self {
            RestartPolicy::Never => None,
            RestartPolicy::Transient {
                max_restarts,
                initial_backoff,
                maximum_backoff,
            } if class == FailureClass::Transient && consecutive_failures <= max_restarts => {
                let exponent = consecutive_failures.saturating_sub(1).min(31);
                let multiplier = 1u32 << exponent;
                Some(initial_backoff.saturating_mul(multiplier).min(maximum_backoff))
            }
            RestartPolicy::Transient {
                max_restarts: _,
                initial_backoff: _,
                maximum_backoff: _,
            } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{failure::FailureClass, restart_policy::RestartPolicy};
    use std::time::Duration;

    #[test]
    fn transient_backoff_is_exponential_and_bounded() {
        let policy = RestartPolicy::transient(4, Duration::from_secs(1), Duration::from_secs(2));
        assert_eq!(policy.delay(FailureClass::Transient, 1), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay(FailureClass::Transient, 2), Some(Duration::from_secs(2)));
        assert_eq!(policy.delay(FailureClass::Transient, 3), Some(Duration::from_secs(2)));
        assert_eq!(policy.delay(FailureClass::Transient, 5), None);
    }

    #[test]
    fn permanent_and_invariant_failures_never_restart() {
        let policy = RestartPolicy::transient(4, Duration::from_secs(1), Duration::from_secs(2));
        assert_eq!(policy.delay(FailureClass::Permanent, 1), None);
        assert_eq!(policy.delay(FailureClass::InvariantViolation, 1), None);
    }
}
