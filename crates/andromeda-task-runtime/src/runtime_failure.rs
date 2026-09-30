//! Failures the task runtime raises itself rather than receives from a task.

use thiserror::Error;
use tokio::task::JoinError;

/// A failure of the runtime's own execution contract.
#[derive(Debug, Error)]
pub enum RuntimeFailure {
    /// A resident operation returned although nothing cancelled it.
    #[error("resident task completed without cancellation")]
    ResidentCompleted,
    /// The run-now channel of a scheduled task closed while the task was active.
    #[error("scheduled run-now channel closed while task was active")]
    RunNowChannelClosed,
    /// The wake channel of a durable drain closed while the task was active.
    #[error("drain wake channel closed while task was active")]
    DrainWakeChannelClosed,
    /// A startup task was registered under a name the supervisor already holds.
    #[error("duplicate startup task registration")]
    DuplicateStartupRegistration,
    /// The spawned execution panicked or was aborted.
    #[error("task execution stopped outside its runtime contract: {0}")]
    ExecutionStopped(#[source] JoinError),
}
