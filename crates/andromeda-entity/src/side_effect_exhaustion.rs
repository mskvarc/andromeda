//! The reason recorded when a side-effect job runs out of attempts silently.

use thiserror::Error;

/// A job whose permitted attempts all ended without settling or releasing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{attempts} attempts ended without settling the job, the configured ceiling is {ceiling}")]
pub(crate) struct SideEffectExhaustion {
    /// Attempts the job had when it was dead-lettered.
    pub(crate) attempts: i32,
    /// The configured number of attempts permitted before dead-lettering.
    pub(crate) ceiling: u16,
}
