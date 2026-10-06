//! The distributed side of a subscription's lifecycle, kept behind a port.
//!
//! Clause 5.8.1.4 has a created subscription provision a companion context
//! source registration subscription, clause 5.8.2.4 has an update follow it,
//! and clause 5.8.5.4 has a deletion tear down both the companion and every
//! copy forwarded to a context source. Subscription policy states those
//! moments here and stays ignorant of registrations, forwarding and transport.
//!
//! A deletion is two moments, not one. The store removes a subscription's
//! forwarding mappings together with the subscription itself, so what the
//! deletion must tear down elsewhere is captured as a [`SubscriptionTeardown`]
//! before the local row goes, and handed back to the hook once it has.

use crate::{deleted_members::DeletedMembers, error::SubscriptionError};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::subscription::Subscription;
use futures::future::BoxFuture;

/// A copy of a local subscription placed at one registration's context source
/// (clause 5.8.1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedCopy {
    /// The registration whose source holds the copy.
    pub registration: Identifier,
    /// The identifier the copy carries at the source.
    pub remote_subscription: Identifier,
}

/// What deleting a subscription must tear down beyond its own row
/// (clause 5.8.5.4), captured while the forwarding mappings still exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionTeardown {
    /// The companion context source registration subscription, if one was linked.
    pub companion: Option<Identifier>,
    /// Every copy forwarded to a context source.
    pub forwarded: Vec<ForwardedCopy>,
}

/// Observes committed entity-subscription lifecycle changes to drive their
/// distributed counterparts.
pub trait SubscriptionForwardingHook: Send + Sync + 'static {
    /// A subscription was created and stored.
    ///
    /// `encountered_sources` are the `Via`-listed context source pseudonyms the
    /// creating request already passed through (Table 6.3.18-2); the hook must
    /// not forward a companion copy back to any of them.
    fn created<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
        encountered_sources: &'life3 [ContextSourcePseudonym],
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// A subscription was updated; `subscription` is its merged state and
    /// `deleted` the members the update removed, which a copy at a context
    /// source still holds (clause 5.8.2.4).
    fn updated<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        deleted: &'life2 DeletedMembers,
        context: &'life3 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// What deleting the subscription `id` must tear down, read before its row
    /// and the mappings that cascade with it are deleted.
    fn teardown_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<SubscriptionTeardown, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The subscription `id` was deleted, and `teardown` is what it left
    /// standing elsewhere.
    fn deleted<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        teardown: &'life2 SubscriptionTeardown,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
