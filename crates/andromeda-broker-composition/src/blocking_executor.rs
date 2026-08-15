//! Tokio binding for CPU-bound policy tasks.

use andromeda_entity::{
    error::EntityError,
    port::blocking_executor::{BlockingExecutor, BlockingTask},
};
use futures::{StreamExt, future::BoxFuture, stream};
use std::num::NonZeroUsize;
use tokio::task::spawn_blocking;

/// Runs policy-supplied CPU work on Tokio's blocking worker pool.
pub struct TokioBlockingExecutor;

impl BlockingExecutor for TokioBlockingExecutor {
    fn execute<'life0, 'async_trait>(
        &'life0 self,
        tasks: Vec<Box<dyn BlockingTask>>,
        concurrency: NonZeroUsize,
    ) -> BoxFuture<'async_trait, Result<(), EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let mut outcomes = stream::iter(tasks)
                .map(|task| spawn_blocking(move || task.run()))
                .buffer_unordered(concurrency.get());
            let mut first_error = None;
            while let Some(outcome) = outcomes.next().await {
                let error = match outcome {
                    Ok(Ok(())) => continue,
                    Ok(Err(error)) => error,
                    Err(error) => EntityError::internal(format!("blocking entity policy task failed: {error}")),
                };
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
    }
}
