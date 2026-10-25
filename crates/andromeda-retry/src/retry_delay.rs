//! Runtime-neutral waiting between attempts.

use futures::future::BoxFuture;
use std::time::Duration;

/// Suspends a retried operation between attempts without exposing an async
/// runtime to the policy that retries it.
///
/// Policy decides *how long* to wait; only the caller knows which timer
/// measures it.
pub trait RetryDelay: Send + Sync + 'static {
    /// Resolve once `delay` has elapsed, yielding to other work meanwhile.
    fn wait<'life0, 'async_trait>(&'life0 self, delay: Duration) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
