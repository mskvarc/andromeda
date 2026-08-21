//! The identifier mappings clause 5.8.1.4 has the broker keep.
//!
//! Two of them are tenant-scoped: a local subscription to its companion context
//! source registration subscription, and a local subscription to the copy
//! forwarded to each registration. The third is platform-wide: every
//! subscription identifier this broker mints and hands to a peer — a companion
//! or a forwarded copy — to the tenant it belongs to, so an inbound
//! notification, which carries no tenant of ours (clause 6.3.14), can still be
//! routed to the right tenant.

use crate::error::SubscriptionForwardingError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::future::BoxFuture;

/// A copy of a local subscription placed at one registration's context source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedCopy {
    /// The registration whose source holds the copy.
    pub registration: Uri,
    /// The identifier the copy carries at the source.
    pub remote_subscription: Uri,
}

/// The tenant-scoped mappings of the distributed subscription side.
pub trait SubscriptionForwardingStore: Send + Sync + 'static {
    /// Record that `companion` is the context source registration subscription
    /// mirroring `local`.
    fn link_companion<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Uri,
        companion: &'life2 Uri,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// The companion of `local`, if one was linked.
    fn companion_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        local: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<Uri>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The local subscription whose companion is `companion`, if any.
    fn local_of_companion<'life0, 'life1, 'async_trait>(
        &'life0 self,
        companion: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<Uri>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Forget the companion of `local`.
    fn unlink_companion<'life0, 'life1, 'async_trait>(&'life0 self, local: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Record the copy of `local` placed at `copy.registration`.
    fn record_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Uri,
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
        local: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Vec<ForwardedCopy>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The local subscription and sending registration behind a copy's identifier.
    fn local_of_forwarded<'life0, 'life1, 'async_trait>(
        &'life0 self,
        remote_subscription: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<(Uri, Uri)>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Forget the copy of `local` at `registration`.
    fn remove_forwarded<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Uri,
        registration: &'life2 Uri,
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
        minted: &'life1 Uri,
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
        minted: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<TenantRef>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Forget `minted`.
    fn unregister<'life0, 'life1, 'async_trait>(&'life0 self, minted: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
