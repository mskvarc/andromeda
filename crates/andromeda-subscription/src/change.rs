//! Cache-invalidation signals for subscription lifecycle changes.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use futures::future::BoxFuture;

/// Signals entity-subscription lifecycle changes.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    /// The subscription with this identifier was created or replaced.
    Upserted {
        /// Branch that owns the subscription.
        target: ContextTarget,
        /// Subscription identifier within the branch.
        id: Identifier,
    },
    /// The subscription with this identifier was deleted.
    Deleted {
        /// Branch that owned the subscription.
        target: ContextTarget,
        /// Deleted subscription identifier.
        id: Identifier,
    },
}

/// Whether an upsert brought a subscription into existence or changed one
/// that already existed.
///
/// A Context Source Registration Subscription is notified differently in the
/// two cases: clause 5.11.2.4 owes a new one a notification "initially on
/// subscription", while clause 5.11.3.4 owes an updated one a notification
/// with "all currently matching Context Source Registrations".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpsertKind {
    /// The subscription was created.
    Created,
    /// An existing subscription was updated.
    Updated,
}

/// Signals context-source-subscription lifecycle changes.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    /// The subscription with this identifier was created or replaced.
    Upserted {
        /// Branch that owns the subscription.
        target: ContextTarget,
        /// Subscription identifier within the branch.
        id: Identifier,
        /// Whether the subscription was created or updated.
        kind: UpsertKind,
    },
    /// The subscription with this identifier was deleted.
    Deleted {
        /// Branch that owned the subscription.
        target: ContextTarget,
        /// Deleted subscription identifier.
        id: Identifier,
    },
}

/// Publishes a lifecycle change without coupling subscription policy to a runtime channel.
pub trait SubscriptionChangeSink<C>: Send + Sync + 'static {
    /// Publish one completed lifecycle change and finish any coherence work
    /// required before a subsequent mutation may observe the new policy.
    fn publish(&self, change: C) -> BoxFuture<'_, ()>;
}
