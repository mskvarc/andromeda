//! Wake-triggered bounded durable drain definition.

use crate::{control::DrainExecution, failure::TaskFailure, registration::TaskRegistration, wake_trigger::DrainFallbackCadence};
use std::{future::Future, pin::Pin, sync::Arc};

/// Whether the runtime should immediately request another bounded pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainPassOutcome {
    /// No immediate additional pass is needed.
    Idle,
    /// The full bounded claim indicates more durable work may remain.
    WorkMayRemain,
}

/// Type-erased bounded operation invoked by drain execution mechanics.
pub(crate) type DrainOperation = dyn Fn(DrainExecution) -> Pin<Box<dyn Future<Output = Result<DrainPassOutcome, TaskFailure>> + Send + 'static>> + Send + Sync;

/// Complete drain task definition accepted by a supervisor.
pub struct DrainTask {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Recovery cadence when no edge wake is observed.
    fallback: DrainFallbackCadence,
    /// Policy operation supplied by composition.
    operation: Arc<DrainOperation>,
}

impl DrainTask {
    /// Adapt a policy-owned bounded drain into runtime mechanics.
    pub fn new<F, Fut>(registration: TaskRegistration, fallback: DrainFallbackCadence, operation: F) -> DrainTask
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
    pub(crate) fn into_parts(self) -> (TaskRegistration, DrainFallbackCadence, Arc<DrainOperation>) {
        (self.registration, self.fallback, self.operation)
    }
}
