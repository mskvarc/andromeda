//! Tokio binding for the wait between contended write attempts.

use andromeda_entity::port::retry_delay::RetryDelay;
use futures::future::BoxFuture;
use std::time::Duration;
use tokio::time::sleep;

/// Holds a contended writer on Tokio's timer wheel, releasing the task's worker
/// thread for the duration.
pub struct TokioRetryDelay;

impl RetryDelay for TokioRetryDelay {
    fn wait<'life0, 'async_trait>(&'life0 self, delay: Duration) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(sleep(delay))
    }
}
