//! A wait that returns at once, for tests of code that retries.

use crate::retry_delay::RetryDelay;
use futures::future::BoxFuture;
use std::time::Duration;

/// Resolves every wait immediately, so a test drives a retry loop through all
/// its attempts without spending the schedule's time.
#[derive(Clone, Copy, Debug, Default)]
pub struct ImmediateRetryDelay;

impl RetryDelay for ImmediateRetryDelay {
    fn wait<'life0, 'async_trait>(&'life0 self, _delay: Duration) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use crate::{immediate_retry_delay::ImmediateRetryDelay, retry_delay::RetryDelay};
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn the_wait_resolves_without_advancing_the_clock() {
        let started = Instant::now();
        ImmediateRetryDelay.wait(Duration::from_secs(3600)).await;
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
