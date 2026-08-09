//! Cache-invalidation signals for subscription lifecycle changes.

use andromeda_ngsi_ld_lexicon::uri::Uri;

/// Signals entity-subscription lifecycle changes.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    /// The subscription with this identifier was created or replaced.
    Upserted(Uri),
    /// The subscription with this identifier was deleted.
    Deleted(Uri),
}

/// Signals context-source-subscription lifecycle changes.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    /// The subscription with this identifier was created or replaced.
    Upserted(Uri),
    /// The subscription with this identifier was deleted.
    Deleted(Uri),
}

/// Publishes a lifecycle change without coupling subscription policy to a runtime channel.
pub trait SubscriptionChangeSink<C>: Send + Sync + 'static {
    /// Publish one completed lifecycle change.
    fn publish(&self, change: C);
}
