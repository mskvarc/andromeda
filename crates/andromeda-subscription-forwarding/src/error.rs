//! Failures of the distributed subscription side.

use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_subscription::error::SubscriptionError;
use thiserror::Error;

/// A failure while mirroring, forwarding or re-assembling a subscription.
#[derive(Debug, Error)]
pub enum SubscriptionForwardingError {
    /// The subscription policy refused or could not complete an operation.
    #[error(transparent)]
    Subscription(#[from] SubscriptionError),
    /// A context source could not be selected or contacted.
    #[error(transparent)]
    Federation(#[from] FederationError),
    /// A local entity read failed during re-assembly.
    #[error(transparent)]
    Entity(#[from] EntityError),
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// A document could not be serialised or read.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// An inbound notification names no subscription this broker forwarded.
    #[error("no local subscription is mapped to {subscription}")]
    UnknownSubscription {
        /// The identifier the notification carried.
        subscription: Uri,
    },
    /// The mapping store could not complete an operation.
    #[error("subscription forwarding persistence failed: {detail}")]
    Persistence {
        /// Backend-neutral diagnostic.
        detail: String,
    },
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// An internal invariant could not be satisfied.
    #[error("subscription forwarding invariant failed: {detail}")]
    Internal {
        /// Diagnostic detail.
        detail: String,
    },
}

impl SubscriptionForwardingError {
    /// Construct a persistence failure.
    pub fn persistence(detail: impl Into<String>) -> SubscriptionForwardingError {
        SubscriptionForwardingError::Persistence { detail: detail.into() }
    }

    /// Construct an internal invariant failure.
    pub fn internal(detail: impl Into<String>) -> SubscriptionForwardingError {
        SubscriptionForwardingError::Internal { detail: detail.into() }
    }
}
