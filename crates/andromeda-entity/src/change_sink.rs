//! Publication boundary for committed entity changes.

use andromeda_context_commit::context_change_event::ContextChangeEvent;
use futures::future::BoxFuture;

/// Publishes committed entity changes without coupling policy to a runtime channel.
pub trait EntityChangeSink: Send + Sync + 'static {
    /// Publish the changes one commit made, in commit order.
    ///
    /// The returned future resolves once every change is handed over, which
    /// may wait while the consumer catches up: clause 5.8.6 owes a notification
    /// for every committed change, so none is dropped. The changes have already
    /// committed, so dropping the future does not withdraw them.
    fn publish(&self, changes: Vec<ContextChangeEvent>) -> BoxFuture<'static, ()>;
}
