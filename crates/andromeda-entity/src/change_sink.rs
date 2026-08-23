//! Publication boundary for committed entity changes.

use andromeda_context_commit::context_change_event::ContextChangeEvent;

/// Publishes a committed entity change without coupling policy to a runtime channel.
pub trait EntityChangeSink: Send + Sync + 'static {
    /// Publish one committed entity change.
    fn publish(&self, event: ContextChangeEvent);
}
