//! Tokio broadcast binding for subscription lifecycle changes.

use andromeda_subscription::change::SubscriptionChangeSink;
use tokio::sync::broadcast;

/// Publishes subscription lifecycle changes through one tenant runtime channel.
pub struct BroadcastSubscriptionChangeSink<C> {
    sender: broadcast::Sender<C>,
}

impl<C> BroadcastSubscriptionChangeSink<C> {
    /// Bind a subscription change sink to the supplied broadcast sender.
    pub const fn new(sender: broadcast::Sender<C>) -> BroadcastSubscriptionChangeSink<C> {
        BroadcastSubscriptionChangeSink { sender }
    }
}

impl<C> SubscriptionChangeSink<C> for BroadcastSubscriptionChangeSink<C>
where
    C: Clone + Send + 'static,
{
    fn publish(&self, change: C) {
        let _receivers = self.sender.send(change);
    }
}
