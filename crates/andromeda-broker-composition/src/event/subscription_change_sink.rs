//! Tokio broadcast binding for subscription lifecycle changes.

use andromeda_subscription::{
    cache::SubscriptionCache,
    change::{SubscriptionChange, SubscriptionChangeSink},
    store::SubscriptionStore,
};
use futures::future::BoxFuture;
use std::sync::Arc;
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
    fn publish<'a>(&'a self, change: C) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _receivers = self.sender.send(change);
        })
    }
}

/// Keeps the entity-subscription matcher coherent before publishing the
/// lifecycle event used by the remaining notification tasks.
pub struct CoherentSubscriptionChangeSink {
    sender: broadcast::Sender<SubscriptionChange>,
    cache: Arc<SubscriptionCache>,
    repository: Arc<dyn SubscriptionStore>,
}

impl CoherentSubscriptionChangeSink {
    pub const fn new(
        sender: broadcast::Sender<SubscriptionChange>,
        cache: Arc<SubscriptionCache>,
        repository: Arc<dyn SubscriptionStore>,
    ) -> CoherentSubscriptionChangeSink {
        CoherentSubscriptionChangeSink { sender, cache, repository }
    }
}

impl SubscriptionChangeSink<SubscriptionChange> for CoherentSubscriptionChangeSink {
    fn publish<'a>(&'a self, change: SubscriptionChange) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.cache.handle_change(&change, self.repository.as_ref()).await;
            let _receivers = self.sender.send(change);
        })
    }
}
