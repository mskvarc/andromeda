//! Tokio binding of the wait between attempts.

use crate::retry_delay::RetryDelay;
use futures::future::BoxFuture;
use std::time::Duration;
use tokio::time::sleep;

/// Holds a retried operation on Tokio's timer wheel, releasing the task's
/// worker thread for the duration.
#[derive(Clone, Copy, Debug, Default)]
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

#[cfg(test)]
mod tests {
    use crate::{retry_delay::RetryDelay, tokio_retry_delay::TokioRetryDelay};
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn the_wait_lasts_the_requested_delay() {
        let started = Instant::now();
        TokioRetryDelay.wait(Duration::from_secs(3)).await;
        assert!(started.elapsed() >= Duration::from_secs(3));
    }
}
