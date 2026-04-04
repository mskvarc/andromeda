pub mod attribute_expiration;
pub mod csr_interval;
pub mod csr_notification;
pub mod entity_expiration;
pub mod expiration;
pub mod interval;
pub mod notification;

/// Trait for background workers with a consistent lifecycle.
///
/// All workers follow the same pattern: construct with `new()`, then
/// call `run()` which consumes `self` and loops until the channel closes.
///
/// Workers implement this trait AND keep their inherent `pub async fn run(self)`
/// method. The inherent method is what `tokio::spawn(worker.run())` calls
/// (inherent methods take priority). The trait enables generic worker management
/// if needed in the future.
#[allow(dead_code)]
#[async_trait::async_trait]
pub trait Worker: Send + 'static {
    async fn run(self);
}
