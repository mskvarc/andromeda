//! The port a subsystem implements to answer for its own health.

use crate::component::{ComponentDetail, ComponentName};
use async_trait::async_trait;

/// A single component health check.
///
/// Implementors report the name they appear under and produce a
/// [`ComponentDetail`] describing current health. The check is expected to
/// answer: it has no timeout of its own and no failure mode other than
/// reporting [`crate::status::HealthStatus::Down`], because a probe that
/// cannot reach its subsystem has already learnt what it was asking.
///
/// Spelled with `async_trait` rather than a native `async fn` because
/// [`crate::aggregate::Health`] holds its indicators as
/// `Arc<dyn HealthIndicator>` — a native `async fn` in a trait is not
/// dyn-compatible, and the alternative is every implementor hand-writing a
/// `Pin<Box<dyn Future>>`.
#[async_trait]
pub trait HealthIndicator: Send + Sync + 'static {
    /// Component name used as the key in the health response.
    fn name(&self) -> ComponentName;

    /// Perform the health check and return component details.
    #[must_use = "health checks must be awaited"]
    async fn check(&self) -> ComponentDetail;
}
