//! Wake-triggered bounded durable drain definition.

use crate::{control::DrainExecution, failure::TaskFailure, registration::TaskRegistration, wake_trigger::DrainFallbackPolicy};
use std::{future::Future, pin::Pin, sync::Arc, time::SystemTime};

/// How a bounded drain pass resolved, deciding when the next pass runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainPassOutcome {
    /// No durable work remained and no next-due time is known; the driver backs
    /// off geometrically toward the fallback ceiling.
    Idle,
    /// The full bounded claim indicates more durable work may remain, so another
    /// pass runs immediately.
    WorkMayRemain,
    /// No work is due now, but the earliest is due at this instant; the driver
    /// sleeps until then, clamped into the fallback band.
    IdleUntil(SystemTime),
}

/// Type-erased bounded operation invoked by drain execution mechanics.
pub(crate) type DrainOperation = dyn Fn(DrainExecution) -> Pin<Box<dyn Future<Output = Result<DrainPassOutcome, TaskFailure>> + Send + 'static>> + Send + Sync;

/// Complete drain task definition accepted by a supervisor.
pub struct DrainTask {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Backoff band recovering from a missed edge wake or process restart.
    fallback: DrainFallbackPolicy,
    /// Policy operation supplied by composition.
    operation: Arc<DrainOperation>,
}

impl DrainTask {
    /// Adapt a policy-owned bounded drain into runtime mechanics.
    pub fn new<F, Fut>(registration: TaskRegistration, fallback: DrainFallbackPolicy, operation: F) -> DrainTask
    where
        F: Fn(DrainExecution) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<DrainPassOutcome, TaskFailure>> + Send + 'static,
    {
        DrainTask {
            registration,
            fallback,
            operation: Arc::new(move |execution| Box::pin(operation(execution))),
        }
    }

    /// Decompose the task for supervisor registration.
    pub(crate) fn into_parts(self) -> (TaskRegistration, DrainFallbackPolicy, Arc<DrainOperation>) {
        (self.registration, self.fallback, self.operation)
    }
}
