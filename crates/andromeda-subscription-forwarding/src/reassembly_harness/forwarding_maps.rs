//! The identifier mappings of a re-assembly test: one local subscription, the
//! copy a source holds of it, and the companion that mirrors it.

use crate::{
    error::SubscriptionForwardingError,
    mapping::{ForwardedSubscriptionRouter, SubscriptionForwardingStore},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_registration::removal_hook::ForwardedSubscriptionCopy;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The mappings of one local subscription, which they name by `local`.
pub(crate) struct ForwardingMaps {
    /// The local subscription.
    pub(crate) local: Identifier,
    /// The registration of the source that holds a copy of it.
    pub(crate) sender: Identifier,
    /// The identifier the copy carries at that source, which its notifications
    /// are posted under.
    pub(crate) copy: Identifier,
    /// The identifier of the companion subscription mirroring `local`.
    pub(crate) companion: Identifier,
    /// The copies of `local` the sources hold, as `forwarded_of` answers them.
    pub(crate) copies: Vec<ForwardedCopy>,
    /// How many times the copies of `local` were asked for.
    copies_asked: AtomicUsize,
}

impl ForwardingMaps {
    /// The mappings naming `local`, its `copy` held at `sender`'s source and its
    /// `companion`, with `copies` placed at the sources.
    pub(crate) const fn new(local: Identifier, sender: Identifier, copy: Identifier, companion: Identifier, copies: Vec<ForwardedCopy>) -> ForwardingMaps {
        ForwardingMaps {
            local,
            sender,
            copy,
            companion,
            copies,
            copies_asked: AtomicUsize::new(0),
        }
    }

    /// How many times the copies of the local subscription were asked for.
    pub(crate) fn copies_asked(&self) -> usize {
        self.copies_asked.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SubscriptionForwardingStore for ForwardingMaps {
    async fn link_companion(&self, _local: &Identifier, _companion: &Identifier) -> Result<(), SubscriptionForwardingError> {
        Ok(())
    }

    async fn companion_of(&self, local: &Identifier) -> Result<Option<Identifier>, SubscriptionForwardingError> {
        Ok((*local == self.local).then(|| self.companion.clone())) // clone: the answer owns the identifier
    }

    async fn local_of_companion(&self, companion: &Identifier) -> Result<Option<Identifier>, SubscriptionForwardingError> {
        Ok((*companion == self.companion).then(|| self.local.clone())) // clone: the answer owns the identifier
    }

    async fn record_forwarded(&self, _local: &Identifier, _copy: &ForwardedCopy) -> Result<(), SubscriptionForwardingError> {
        Ok(())
    }

    async fn forwarded_of(&self, local: &Identifier) -> Result<Vec<ForwardedCopy>, SubscriptionForwardingError> {
        self.copies_asked.fetch_add(1, Ordering::SeqCst);
        Ok(if *local == self.local { self.copies.clone() } else { Vec::new() }) // clone: the answer owns the copies
    }

    async fn forwarded_for_registration(&self, _registration: &Identifier) -> Result<Vec<ForwardedSubscriptionCopy>, SubscriptionForwardingError> {
        Ok(Vec::new())
    }

    async fn local_of_forwarded(&self, remote_subscription: &Identifier) -> Result<Option<(Identifier, Identifier)>, SubscriptionForwardingError> {
        // clone: the answer owns both identifiers
        Ok((*remote_subscription == self.copy).then(|| (self.local.clone(), self.sender.clone())))
    }

    async fn remove_forwarded(&self, _local: &Identifier, _registration: &Identifier) -> Result<(), SubscriptionForwardingError> {
        Ok(())
    }
}

/// A router that accepts every identifier and knows none.
pub(crate) struct DiscardingRouter;

#[async_trait]
impl ForwardedSubscriptionRouter for DiscardingRouter {
    async fn register(&self, _minted: &Identifier, _tenant: &TenantRef) -> Result<(), SubscriptionForwardingError> {
        Ok(())
    }

    async fn tenant_of(&self, _minted: &Identifier) -> Result<Option<TenantRef>, SubscriptionForwardingError> {
        Ok(None)
    }

    async fn unregister(&self, _minted: &Identifier) -> Result<(), SubscriptionForwardingError> {
        Ok(())
    }
}
