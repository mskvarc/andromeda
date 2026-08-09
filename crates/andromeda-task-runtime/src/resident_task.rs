//! Resident task definition for long-lived asynchronous processing.

use crate::{control::ResidentExecution, failure::TaskFailure, registration::TaskRegistration};
use std::{future::Future, pin::Pin, sync::Arc};

/// Type-erased resident operation invoked by task execution mechanics.
pub(crate) type ResidentOperation = dyn Fn(ResidentExecution) -> Pin<Box<dyn Future<Output = Result<(), TaskFailure>> + Send + 'static>> + Send + Sync;

/// Complete resident task definition accepted by a supervisor.
pub struct ResidentTask {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Long-lived operation supplied by composition.
    operation: Arc<ResidentOperation>,
}

impl ResidentTask {
    /// Adapt a long-lived operation into one resident task definition.
    pub fn new<F, Fut>(registration: TaskRegistration, operation: F) -> ResidentTask
    where
        F: Fn(ResidentExecution) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), TaskFailure>> + Send + 'static,
    {
        ResidentTask {
            registration,
            operation: Arc::new(move |execution| Box::pin(operation(execution))),
        }
    }

    /// Decompose the task for supervisor registration.
    pub(crate) fn into_parts(self) -> (TaskRegistration, Arc<ResidentOperation>) {
        (self.registration, self.operation)
    }
}
