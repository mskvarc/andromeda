//! Runtime-neutral execution of CPU-bound entity policy work.

use crate::error::EntityError;
use futures::future::BoxFuture;
use std::num::NonZeroUsize;

/// One CPU-bound operation prepared by entity policy for a runtime executor.
pub trait BlockingTask: Send + 'static {
    /// Run the operation to completion on a blocking worker thread.
    fn run(self: Box<Self>) -> Result<(), EntityError>;
}

/// Executes CPU-bound policy tasks without exposing an async runtime to entity policy.
pub trait BlockingExecutor: Send + Sync + 'static {
    /// Execute every task with at most `concurrency` tasks in flight.
    fn execute<'life0, 'async_trait>(
        &'life0 self,
        tasks: Vec<Box<dyn BlockingTask>>,
        concurrency: NonZeroUsize,
    ) -> BoxFuture<'async_trait, Result<(), EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
