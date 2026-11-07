//! Which bound ended a task's cooperative shutdown.

use std::time::Duration;
use strum::Display;

/// The bound a stopping task ran into before it stopped on its own.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum ShutdownCutoff {
    /// The task's own grace period ran out.
    GracePeriod,
    /// The deadline the whole scope was given ran out first.
    Deadline,
}

impl ShutdownCutoff {
    /// How long a task may still take, and the bound that ends it: the
    /// smaller of what is left of its grace period and, when the scope has a
    /// deadline, what is left until it. A tie is the grace period's, which
    /// would have ended the task without the deadline.
    pub(crate) fn remaining(grace_left: Duration, deadline_left: Option<Duration>) -> (Duration, ShutdownCutoff) {
        match deadline_left {
            Some(deadline_left) if deadline_left < grace_left => (deadline_left, ShutdownCutoff::Deadline),
            Some(_) | None => (grace_left, ShutdownCutoff::GracePeriod),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::shutdown_cutoff::ShutdownCutoff;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_nearer_bound_ends_a_task_and_a_tie_is_the_grace_periods() {
        let second = Duration::from_secs(1);
        let minute = Duration::from_secs(60);
        assert_eq!(ShutdownCutoff::remaining(minute, Some(second)), (second, ShutdownCutoff::Deadline));
        assert_eq!(ShutdownCutoff::remaining(second, Some(minute)), (second, ShutdownCutoff::GracePeriod));
        assert_eq!(ShutdownCutoff::remaining(second, Some(second)), (second, ShutdownCutoff::GracePeriod));
        assert_eq!(ShutdownCutoff::remaining(minute, None), (minute, ShutdownCutoff::GracePeriod));
    }
}
