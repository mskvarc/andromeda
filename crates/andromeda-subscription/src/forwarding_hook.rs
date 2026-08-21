//! The distributed side of a subscription's lifecycle, kept behind a port.
//!
//! Clause 5.8.1.4 has a created subscription provision a companion context
//! source registration subscription, clause 5.8.2.4 has an update follow it,
//! and clause 5.8.5.4 has a deletion tear down both the companion and every
//! copy forwarded to a context source. Subscription policy states those three
//! moments here and stays ignorant of registrations, forwarding and transport.

use crate::error::SubscriptionError;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::Subscription;
use futures::future::BoxFuture;

/// Observes committed entity-subscription lifecycle changes to drive their
/// distributed counterparts.
pub trait SubscriptionForwardingHook: Send + Sync + 'static {
    /// A subscription was created and stored.
    fn created<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// A subscription was updated; `subscription` is its merged state.
    fn updated<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// A subscription was deleted.
    fn deleted<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
