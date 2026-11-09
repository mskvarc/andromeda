//! The platform route of a re-assembly test. It records every identifier
//! routed and unrouted, resolves none to a tenant, and can hold the routing of
//! an identifier until the test lets it go on, so a test can cut a placement
//! off between its claim and its send.

use crate::{error::SubscriptionForwardingError, mapping::ForwardedSubscriptionRouter};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

/// A router that records the identifiers it is told to route and to forget.
#[derive(Default)]
pub(crate) struct RecordingRouter {
    /// The identifiers routed, in the order they were.
    registered: Mutex<Vec<Identifier>>,
    /// The identifiers forgotten, in the order they were.
    unregistered: Mutex<Vec<Identifier>>,
    /// Whether routing waits until it is let go on.
    held: AtomicBool,
    /// Wakes the routings waiting once they are let go on.
    released: Notify,
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

    /// Hold every routing from now on, before it records its identifier.
    pub(crate) fn hold_routing(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    /// Let every held routing, and every later one, go on.
    pub(crate) fn release_routing(&self) {
        self.held.store(false, Ordering::SeqCst);
        self.released.notify_waiters();
    }

    /// Wait until routing is not held.
    async fn while_held(&self) {
        loop {
            // Registered as a waiter before the flag is read, so a release
            // between the read and the wait still wakes it.
            let released = self.released.notified();
            tokio::pin!(released);
            released.as_mut().enable();
            if !self.held.load(Ordering::SeqCst) {
                return;
            }
            released.await;
        }
    }
}

#[async_trait]
impl ForwardedSubscriptionRouter for RecordingRouter {
    async fn register(&self, minted: &Identifier, _tenant: &TenantRef) -> Result<(), SubscriptionForwardingError> {
        self.while_held().await;
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
