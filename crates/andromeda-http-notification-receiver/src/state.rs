//! Resolution of the tenant a received notification continues in.
//!
//! A notification carries no tenant of ours: clause 6.3.14 makes its
//! `NGSILD-Tenant` the sender's. The subscription identifier it names is one
//! this broker minted, and that identifier is what selects the tenant.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription_forwarding::service::SubscriptionForwardingService;
use andromeda_tenancy::runtime::lease::RuntimeLease;
use futures::future::BoxFuture;
use std::sync::Arc;

/// A tenant's forwarding service together with the lease that keeps its runtime
/// resident while the received notification is handled.
pub struct LeasedForwarding {
    /// The service handling notifications for the resolved subscription.
    pub service: Arc<SubscriptionForwardingService>,
    /// Keeps the resolved tenant's runtime resident until handling completes,
    /// so eviction never closes its pools mid-notification.
    pub lease: RuntimeLease,
}

/// Why a minted subscription identifier could not be resolved to a tenant's service.
#[derive(Debug, thiserror::Error)]
pub enum ReceiverResolutionError {
    /// No tenant minted this identifier.
    #[error("no forwarded subscription is known under {subscription}")]
    Unknown {
        /// The identifier the notification named.
        subscription: Identifier,
    },
    /// The tenant exists but its distributed side is not configured.
    #[error("subscription forwarding is not enabled for the tenant of {subscription}")]
    ForwardingDisabled {
        /// The identifier the notification named.
        subscription: Identifier,
    },
    /// The tenant could not be resolved.
    #[error("tenant resolution failed: {detail}")]
    Tenant {
        /// Diagnostic detail.
        detail: String,
    },
}

/// Resolves the forwarding service of the tenant that minted a subscription identifier.
pub trait NotificationReceiverResolver: Send + Sync {
    /// The service handling notifications for `subscription`, leased so the
    /// tenant runtime stays resident while the notification is handled.
    fn resolve<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<LeasedForwarding, ReceiverResolutionError>>;
}

/// Axum state for the receiver route.
#[derive(Clone)]
pub(crate) struct NotificationReceiverState {
    /// The resolver the handler asks.
    pub(crate) resolver: Arc<dyn NotificationReceiverResolver>,
}
