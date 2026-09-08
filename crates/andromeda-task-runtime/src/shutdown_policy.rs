//! Cooperative shutdown grace periods.

use std::time::Duration;

/// Maximum duration a task may use to stop cooperatively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownPolicy {
    /// Time allowed for cooperative termination.
    grace_period: Duration,
}

impl ShutdownPolicy {
    /// Construct a shutdown policy with the supplied grace period.
    #[must_use]
    pub const fn new(grace_period: Duration) -> ShutdownPolicy {
        ShutdownPolicy { grace_period }
    }

    /// Return the cooperative grace period.
    #[must_use]
    pub const fn grace_period(self) -> Duration {
        self.grace_period
    }
}
