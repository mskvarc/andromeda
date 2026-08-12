//! Runtime-neutral waiting between contended write attempts.

use futures::future::BoxFuture;
use std::time::Duration;

/// Suspends a contended write between attempts without exposing an async
/// runtime to entity policy.
///
/// Policy decides *how long* two colliding writers must be held apart; only the
/// caller knows which timer measures it, exactly as it alone knows which pool
/// runs [`BlockingExecutor`](crate::port::blocking_executor::BlockingExecutor).
pub trait RetryDelay: Send + Sync + 'static {
    /// Resolve once `delay` has elapsed, yielding to other work meanwhile.
    fn wait<'life0, 'async_trait>(&'life0 self, delay: Duration) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
