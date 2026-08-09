//! Finite startup task definition and failure reporting.

use crate::{failure::TaskFailure, registration::TaskRegistration, task_name::TaskName};
use std::{future::Future, pin::Pin, sync::Arc};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Type-erased finite startup operation.
pub(crate) type StartupOperation = dyn Fn(CancellationToken) -> Pin<Box<dyn Future<Output = Result<(), TaskFailure>> + Send + 'static>> + Send + Sync;

/// Complete finite startup task definition.
pub struct StartupTask {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Finite construction operation supplied by composition.
    operation: Arc<StartupOperation>,
}

impl StartupTask {
    /// Adapt a finite operation into one startup task definition.
    pub fn new<F, Fut>(registration: TaskRegistration, operation: F) -> StartupTask
    where
        F: Fn(CancellationToken) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), TaskFailure>> + Send + 'static,
    {
        StartupTask {
            registration,
            operation: Arc::new(move |cancellation| Box::pin(operation(cancellation))),
        }
    }

    /// Decompose the task for supervisor execution.
    pub(crate) fn into_parts(self) -> (TaskRegistration, Arc<StartupOperation>) {
        (self.registration, self.operation)
    }
}

/// A startup task failed and the constructed resource must remain hidden.
#[derive(Clone, Debug, Error)]
#[error("startup task `{name}` failed: {failure}")]
pub struct StartupTaskError {
    /// Name of the failed startup task.
    pub name: TaskName,
    /// Classified task failure.
    pub failure: TaskFailure,
}
