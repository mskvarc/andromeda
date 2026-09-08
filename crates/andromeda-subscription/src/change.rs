//! Cache-invalidation signals for subscription lifecycle changes.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use futures::future::BoxFuture;

/// Signals entity-subscription lifecycle changes.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    /// The subscription with this identifier was created or replaced.
    Upserted {
        /// Branch that owns the subscription.
        target: ContextTarget,
        /// Subscription identifier within the branch.
        id: Uri,
    },
    /// The subscription with this identifier was deleted.
    Deleted {
        /// Branch that owned the subscription.
        target: ContextTarget,
        /// Deleted subscription identifier.
        id: Uri,
    },
}

/// Signals context-source-subscription lifecycle changes.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    /// The subscription with this identifier was created or replaced.
    Upserted {
        /// Branch that owns the subscription.
        target: ContextTarget,
        /// Subscription identifier within the branch.
        id: Uri,
    },
    /// The subscription with this identifier was deleted.
    Deleted {
        /// Branch that owned the subscription.
        target: ContextTarget,
        /// Deleted subscription identifier.
        id: Uri,
    },
}

/// Publishes a lifecycle change without coupling subscription policy to a runtime channel.
pub trait SubscriptionChangeSink<C>: Send + Sync + 'static {
    /// Publish one completed lifecycle change and finish any coherence work
    /// required before a subsequent mutation may observe the new policy.
    fn publish(&self, change: C) -> BoxFuture<'_, ()>;
}
