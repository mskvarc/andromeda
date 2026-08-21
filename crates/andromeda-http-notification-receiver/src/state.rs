//! Resolution of the tenant a received notification continues in.
//!
//! A notification carries no tenant of ours: clause 6.3.14 makes its
//! `NGSILD-Tenant` the sender's. The subscription identifier it names is one
//! this broker minted, and that identifier is what selects the tenant.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_subscription_forwarding::service::SubscriptionForwardingService;
use futures::future::BoxFuture;
use std::sync::Arc;

/// Why a minted subscription identifier could not be resolved to a tenant's service.
#[derive(Debug, thiserror::Error)]
pub enum ReceiverResolutionError {
    /// No tenant minted this identifier.
    #[error("no forwarded subscription is known under {subscription}")]
    Unknown {
        /// The identifier the notification named.
        subscription: Uri,
    },
    /// The tenant exists but its distributed side is not configured.
    #[error("subscription forwarding is not enabled for the tenant of {subscription}")]
    ForwardingDisabled {
        /// The identifier the notification named.
        subscription: Uri,
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
    /// The service handling notifications for `subscription`.
    fn resolve<'a>(&'a self, subscription: &'a Uri) -> BoxFuture<'a, Result<Arc<SubscriptionForwardingService>, ReceiverResolutionError>>;
}

/// Axum state for the receiver route.
#[derive(Clone)]
pub(crate) struct NotificationReceiverState {
    /// The resolver the handler asks.
    pub(crate) resolver: Arc<dyn NotificationReceiverResolver>,
}
