//! How often, and how far apart, an operation is run again after a retryable
//! failure.

use crate::{retry_delay::RetryDelay, retry_number::RetryNumber, retry_run::RetryRun, schedule::BackoffSchedule};
use std::num::NonZeroU32;

/// Which run of an operation is starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryAttempt {
    /// The first run, which follows no wait.
    First,
    /// A run after a lost attempt, and the wait the schedule held it for.
    Retry(RetryNumber),
}

/// How often, and how far apart, a failed operation is run again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    /// Runs in all, the first included.
    pub attempts: NonZeroU32,
    /// The wait before each run after the first.
    pub schedule: BackoffSchedule,
}

impl RetryPolicy {
    /// Start a run of the operation this policy retries, whose waits `delay`
    /// measures.
    #[must_use]
    pub const fn start<'d>(&self, delay: &'d dyn RetryDelay) -> RetryRun<'d> {
        RetryRun::new(*self, delay)
    }
}
