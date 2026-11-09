//! The platform route of a re-assembly test: it records every identifier
//! routed and unrouted, and resolves none to a tenant.

use crate::{error::SubscriptionForwardingError, mapping::ForwardedSubscriptionRouter};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use std::sync::Mutex;

/// A router that records the identifiers it is told to route and to forget.
#[derive(Default)]
pub(crate) struct RecordingRouter {
    /// The identifiers routed, in the order they were.
    registered: Mutex<Vec<Identifier>>,
    /// The identifiers forgotten, in the order they were.
    unregistered: Mutex<Vec<Identifier>>,
}

impl RecordingRouter {
    /// The identifiers routed, in the order they were.
    pub(crate) fn registered(&self) -> Vec<Identifier> {
        self.registered.lock().expect("the routes are never poisoned").clone()
    }

    /// The identifiers forgotten, in the order they were.
    pub(crate) fn unregistered(&self) -> Vec<Identifier> {
        self.unregistered.lock().expect("the forgotten routes are never poisoned").clone()
    }
}

#[async_trait]
impl ForwardedSubscriptionRouter for RecordingRouter {
    async fn register(&self, minted: &Identifier, _tenant: &TenantRef) -> Result<(), SubscriptionForwardingError> {
        self.registered.lock().expect("the routes are never poisoned").push(minted.clone());
        Ok(())
    }

    async fn tenant_of(&self, _minted: &Identifier) -> Result<Option<TenantRef>, SubscriptionForwardingError> {
        Ok(None)
    }

    async fn unregister(&self, minted: &Identifier) -> Result<(), SubscriptionForwardingError> {
        self.unregistered.lock().expect("the forgotten routes are never poisoned").push(minted.clone());
        Ok(())
    }
}
