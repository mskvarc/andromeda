//! Failures of the distributed subscription side.

use crate::forwarding_invariant::ForwardingInvariant;
use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_registration::error::RegistrationError;
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
    /// The registration store could not be read.
    #[error(transparent)]
    Registration(#[from] RegistrationError),
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
        subscription: Identifier,
    },
    /// The mapping store could not complete an operation.
    #[error("subscription forwarding persistence failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore is temporarily unavailable; retry shortly")]
    StoreUnavailable,
    /// An internal invariant could not be satisfied.
    #[error("subscription forwarding invariant failed: {0}")]
    Internal(#[from] ForwardingInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a store that cannot be reached are conditions the
/// client can retry, so each keeps its own retryable category; every other
/// failure is a persistence fault.
impl From<PersistenceFailure> for SubscriptionForwardingError {
    fn from(failure: PersistenceFailure) -> SubscriptionForwardingError {
        match failure {
            PersistenceFailure::Saturated => SubscriptionForwardingError::StoreSaturated,
            PersistenceFailure::Unavailable => SubscriptionForwardingError::StoreUnavailable,
            fault @ (PersistenceFailure::RolledBack
            | PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::CapacityExceeded
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => SubscriptionForwardingError::Persistence(fault),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::SubscriptionForwardingError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(
            SubscriptionForwardingError::from(PersistenceFailure::Unavailable),
            SubscriptionForwardingError::StoreUnavailable
        ));
        assert!(matches!(
            SubscriptionForwardingError::from(PersistenceFailure::Misconfigured),
            SubscriptionForwardingError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
