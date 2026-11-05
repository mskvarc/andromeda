//! Tokio broadcast binding for subscription lifecycle changes.

use andromeda_subscription::{
    cache::SubscriptionCache,
    change::{SubscriptionChange, SubscriptionChangeSink},
    store::SubscriptionStore,
};
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, warn};

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
    fn publish(&self, change: C) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let _receivers = self.sender.send(change);
        })
    }
}

/// Keeps the entity-subscription matcher coherent before publishing the
/// lifecycle event used by the remaining notification tasks.
///
/// The sink is the one writer of the cache for a subscription change and makes
/// a single read: a request waits on it, so a read whose store cannot be
/// reached is left to the cache's reconcile pass instead of being retried here.
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
    fn publish(&self, change: SubscriptionChange) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            match self.cache.handle_change(&change, self.repository.as_ref()).await {
                Ok(()) => {}
                Err(error) if error.is_transient() => warn!(
                    ?change,
                    "Subscription cache could not reach the store for a committed change; the periodic reconcile reads it again: {error}"
                ),
                Err(error) => error!(
                    ?change,
                    "Subscription cache could not read a committed change; it stays as cached until it changes again or the cache reloads: {error}"
                ),
            }
            let _receivers = self.sender.send(change);
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::event::subscription_change_sink::CoherentSubscriptionChangeSink;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
    use andromeda_subscription::{
        answering_store::{AnsweringStore, StoredAnswer},
        cache::SubscriptionCache,
        change::{SubscriptionChange, SubscriptionChangeSink},
        store::SubscriptionStore,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use pretty_assertions::assert_eq;
    use std::{error::Error, sync::Arc};
    use tokio::sync::broadcast;

    fn upserted(id: &Identifier) -> SubscriptionChange {
        SubscriptionChange::Upserted {
            target: ContextTarget::Live,
            id: id.clone(), // clone: the change owns its identifier
        }
    }

    fn sink(store: &Arc<AnsweringStore>) -> (CoherentSubscriptionChangeSink, Arc<SubscriptionCache>, broadcast::Receiver<SubscriptionChange>) {
        let (sender, receiver) = broadcast::channel(4);
        let cache = Arc::new(SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality));
        let sink = CoherentSubscriptionChangeSink::new(
            sender,
            Arc::clone(&cache), // clone: the test inspects the cache the sink writes
            Arc::clone(store) as Arc<dyn SubscriptionStore>,
        );
        (sink, cache, receiver)
    }

    #[tokio::test]
    async fn a_publish_reads_the_subscription_once_and_indexes_it() -> Result<(), Box<dyn Error>> {
        let store = Arc::new(AnsweringStore::new(StoredAnswer::UnreadableForReads(0)));
        let (sink, cache, mut receiver) = sink(&store);
        let id: Identifier = "urn:ngsi-ld:Subscription:published".parse()?;

        sink.publish(upserted(&id)).await;

        assert_eq!(store.reads(), 1);
        assert!(cache.get(&ContextTarget::Live, &id).is_some());
        assert!(receiver.try_recv().is_ok(), "the change is broadcast");
        Ok(())
    }

    #[tokio::test]
    async fn a_publish_whose_read_fails_reads_once_marks_the_subscription_dirty_and_still_broadcasts() -> Result<(), Box<dyn Error>> {
        let store = Arc::new(AnsweringStore::new(StoredAnswer::Unreadable));
        let (sink, cache, mut receiver) = sink(&store);
        let id: Identifier = "urn:ngsi-ld:Subscription:unreadable".parse()?;

        sink.publish(upserted(&id)).await;

        assert_eq!(store.reads(), 1);
        assert_eq!(cache.dirty_len(), 1);
        assert!(receiver.try_recv().is_ok(), "the change is broadcast although the refresh failed");
        Ok(())
    }
}
