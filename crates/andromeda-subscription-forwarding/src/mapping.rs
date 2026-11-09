//! The identifier mappings clause 5.8.1.4 has the broker keep.
//!
//! Two of them are tenant-scoped: a local subscription to its companion context
//! source registration subscription, and a local subscription to the copy
//! forwarded to each registration. The third is platform-wide: every
//! subscription identifier this broker mints and hands to a peer (a companion
//! or a forwarded copy) to the tenant it belongs to, so an inbound
//! notification, which carries no tenant of ours (clause 6.3.14), can still be
//! routed to the right tenant.

use crate::{
    error::SubscriptionForwardingError,
    forwarded_claim::{ForwardedClaim, StaleClaim},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_registration::removal_hook::ForwardedSubscriptionCopy;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::future::BoxFuture;
use std::time::Duration;

/// The tenant-scoped mappings of the distributed subscription side.
///
/// A store removes a subscription's mappings together with the subscription,
/// and a registration's forwarded copies together with the registration, so
/// forgetting either explicitly is never needed after a deletion; only a copy
/// withdrawn while both ends still exist is removed by hand.
pub trait SubscriptionForwardingStore: Send + Sync + 'static {
    /// Record that `companion` is the context source registration subscription
    /// mirroring `local`.
    fn link_companion<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        companion: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// The companion of `local`, if one was linked.
    fn companion_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Identifier>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The local subscription whose companion is `companion`, if any.
    fn local_of_companion<'life0, 'life1, 'async_trait>(
        &'life0 self,
        companion: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Identifier>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Record `copy` as the copy of `local` at `copy.registration` before it is
    /// sent there, under a lease of `lease` from now.
    ///
    /// A copy of `local` already recorded at the registration whose lease has
    /// run out is taken over instead. Its lease starts again and the claim
    /// answers the identifier it was recorded with. Nothing is recorded when a
    /// copy there is settled or still under its lease, or when either `local`
    /// or the registration no longer exists.
    fn claim_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        copy: &'life2 ForwardedCopy,
        lease: Duration,
    ) -> BoxFuture<'async_trait, Result<ForwardedClaim, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Settle the claimed copy of `local` at `registration`, now that its
    /// source holds it, so its lease no longer runs.
    fn settle_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        registration: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Up to `limit` claims whose lease has run out before their source's
    /// answer settled them, the longest overdue first.
    fn stale_claims<'life0, 'async_trait>(&'life0 self, limit: u32) -> BoxFuture<'async_trait, Result<Vec<StaleClaim>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Record the copy of `local` its source holds at `copy.registration`,
    /// settled, replacing the identifier the copy recorded there carried.
    fn record_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        copy: &'life2 ForwardedCopy,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Every copy of `local`.
    fn forwarded_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Vec<ForwardedCopy>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Every copy held at `registration`'s context source, with the local
    /// subscription each reduces.
    fn forwarded_for_registration<'life0, 'life1, 'async_trait>(
        &'life0 self,
        registration: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Vec<ForwardedSubscriptionCopy>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The local subscription and sending registration behind a copy's identifier.
    fn local_of_forwarded<'life0, 'life1, 'async_trait>(
        &'life0 self,
        remote_subscription: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<(Identifier, Identifier)>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Forget the copy of `local` at `registration`.
    fn remove_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        registration: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}

/// The platform-wide route from a minted subscription identifier to its tenant.
pub trait ForwardedSubscriptionRouter: Send + Sync + 'static {
    /// Record that `minted` belongs to `tenant`.
    fn register<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        minted: &'life1 Identifier,
        tenant: &'life2 TenantRef,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// The tenant `minted` belongs to, if this broker minted it.
    fn tenant_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        minted: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<TenantRef>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Forget `minted`.
    fn unregister<'life0, 'life1, 'async_trait>(&'life0 self, minted: &'life1 Identifier) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
