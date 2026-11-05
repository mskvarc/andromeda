//! Retry policy: how long to wait before an operation is run again, and the
//! loop that runs it.
//!
//! One law serves every retry in the broker: a first delay grows by a
//! [`Growth`](crate::growth::Growth) multiplier, saturates at a ceiling and is
//! then spread by a [`Jitter`](crate::jitter::Jitter). The law is evaluated at
//! any retry number, because several queues persist their attempt count and
//! read the delay back from it, and also walked by a cursor or a bounded loop
//! for the callers that hold the count in memory. Each caller keeps only what
//! it alone knows: what is retryable, when a streak resets, where the count is
//! stored and which timer waits.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// A stateful walk through a schedule for an in-memory failure streak.
pub mod cursor;
/// How the delay grows from one retry to the next.
pub mod growth;
/// A wait that returns at once, for tests.
#[cfg(feature = "test-support")]
pub mod immediate_retry_delay;
/// How a delay is spread below or above its bound.
pub mod jitter;
/// The runtime-neutral port that waits out a delay.
pub mod retry_delay;
/// The 1-based ordinal of a retry.
pub mod retry_number;
/// The bounded loop that runs an operation again.
pub mod retry_policy;
/// The delay before any retry, evaluated at its retry number.
pub mod schedule;
/// Why a configured schedule is refused.
pub mod schedule_error;
/// The Tokio binding of the wait port.
#[cfg(feature = "tokio")]
pub mod tokio_retry_delay;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["fastrand", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
