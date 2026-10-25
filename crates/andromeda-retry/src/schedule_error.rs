//! Why a schedule read from operator configuration is refused.

use thiserror::Error;

/// A schedule whose bounds cannot mean what its operator wrote.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BackoffScheduleError {
    /// The first delay is zero, so the retries would never wait.
    #[error("the first retry delay must be greater than zero")]
    ZeroFirst,
    /// The ceiling is shorter than the first delay it is meant to cap.
    #[error("the retry delay ceiling must not be shorter than the first delay")]
    CeilingBelowFirst,
}
