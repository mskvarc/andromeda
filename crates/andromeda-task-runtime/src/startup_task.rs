//! Finite startup task definition and failure reporting.

use crate::{failure::TaskFailure, registration::TaskRegistration, runtime_failure::RuntimeFailure, task_name::TaskName};
use std::{future::Future, pin::Pin, sync::Arc};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Type-erased finite startup operation failing with `E`.
pub(crate) type StartupOperation<E> = dyn Fn(CancellationToken) -> Pin<Box<dyn Future<Output = Result<(), TaskFailure<E>>> + Send + 'static>> + Send + Sync;

/// Complete finite startup task definition whose operation fails with `E`.
pub struct StartupTask<E> {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Finite construction operation supplied by composition.
    operation: Arc<StartupOperation<E>>,
}

impl<E> StartupTask<E> {
    /// Adapt a finite operation into one startup task definition.
    pub fn new<F, Fut>(registration: TaskRegistration, operation: F) -> StartupTask<E>
    where
        F: Fn(CancellationToken) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), TaskFailure<E>>> + Send + 'static,
    {
        StartupTask {
            registration,
            operation: Arc::new(move |cancellation| Box::pin(operation(cancellation))),
        }
    }

    /// Decompose the task for supervisor execution.
    pub(crate) fn into_parts(self) -> (TaskRegistration, Arc<StartupOperation<E>>) {
        (self.registration, self.operation)
    }
}

/// Why a startup task did not complete.
#[derive(Debug, Error)]
pub enum StartupFailure<E> {
    /// The operation reported a classified failure of its own.
    #[error(transparent)]
    Operation(TaskFailure<E>),
    /// The runtime could not run the operation to completion.
    #[error(transparent)]
    Runtime(TaskFailure<RuntimeFailure>),
}

/// A startup task failed and the constructed resource must remain hidden.
#[derive(Debug, Error)]
#[error("startup task `{name}` failed: {failure}")]
pub struct StartupTaskError<E> {
    /// Name of the failed startup task.
    pub name: TaskName,
    /// Why the task did not complete.
    #[source]
    pub failure: StartupFailure<E>,
}
