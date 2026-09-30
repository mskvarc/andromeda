//! Resident task definition for long-lived asynchronous processing.

use crate::{control::ResidentExecution, failure::TaskFailure, failure_report::FailureReport, registration::TaskRegistration};
use std::{error::Error, future::Future, pin::Pin, sync::Arc};

/// Type-erased resident operation invoked by task execution mechanics.
///
/// The operation's typed failure is rendered into a [`FailureReport`] where it
/// is adapted, so drivers of unrelated tasks share one operation shape.
pub(crate) type ResidentOperation = dyn Fn(ResidentExecution) -> Pin<Box<dyn Future<Output = Result<(), FailureReport>> + Send + 'static>> + Send + Sync;

/// Complete resident task definition accepted by a supervisor.
pub struct ResidentTask {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Long-lived operation supplied by composition.
    operation: Arc<ResidentOperation>,
}

impl ResidentTask {
    /// Adapt a long-lived operation into one resident task definition.
    pub fn new<F, Fut, E>(registration: TaskRegistration, operation: F) -> ResidentTask
    where
        F: Fn(ResidentExecution) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), TaskFailure<E>>> + Send + 'static,
        E: Error,
    {
        ResidentTask {
            registration,
            operation: Arc::new(move |execution| {
                let attempt = operation(execution);
                Box::pin(async move { attempt.await.map_err(|failure| FailureReport::of(&failure)) })
            }),
        }
    }

    /// Decompose the task for supervisor registration.
    pub(crate) fn into_parts(self) -> (TaskRegistration, Arc<ResidentOperation>) {
        (self.registration, self.operation)
    }
}
