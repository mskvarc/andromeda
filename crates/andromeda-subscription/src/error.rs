//! Errors produced by subscription policy and persistence.

use crate::{subscription_invariant::SubscriptionInvariant, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_jsonld::{compound_context_host::ContextHostingError, error::JsonLdError};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryOverLimit;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use thiserror::Error;

/// A failure while validating, applying, compiling, or persisting a subscription.
#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// The request does not satisfy the subscription contract.
    #[error("invalid subscription: {0}")]
    Validation(#[from] SubscriptionValidationReason),
    /// The requested subscription does not exist.
    #[error("subscription not found: {identifier}")]
    NotFound {
        /// Missing subscription identifier.
        identifier: Identifier,
    },
    /// A subscription already uses the requested identifier.
    #[error("subscription already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting subscription identifier.
        identifier: Identifier,
    },
    /// The `geoQ` reference geometry holds more positions, or more JSON values
    /// besides them, than the broker accepts in one geometry (clause 6.3.2,
    /// "Request Entity too large").
    #[error("the geoQ geometry holds {over}; send a simplified geometry")]
    GeoQTooLarge {
        /// The limit the geometry holds more than.
        over: GeometryOverLimit,
    },
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// The `@context` applicable at creation could not be hosted, so no
    /// `jsonldContext` can name it (clause 5.8.1.4).
    #[error("the subscription's applicable @context could not be hosted: {0}")]
    ContextHosting(#[from] ContextHostingError),
    /// Query parsing or expansion failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// The persistence adapter could not complete the operation.
    #[error("subscription persistence failed: {0}")]
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
    /// An internal subscription invariant could not be satisfied.
    #[error("subscription invariant failed: {0}")]
    Internal(#[from] SubscriptionInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a store that cannot be reached are conditions the
/// client can retry, so each keeps its own retryable category; every other
/// failure is a persistence fault.
impl From<PersistenceFailure> for SubscriptionError {
    fn from(failure: PersistenceFailure) -> SubscriptionError {
        match failure {
            PersistenceFailure::Saturated => SubscriptionError::StoreSaturated,
            PersistenceFailure::Unavailable => SubscriptionError::StoreUnavailable,
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
            | PersistenceFailure::ContractBroken) => SubscriptionError::Persistence(fault),
        }
    }
}

impl SubscriptionError {
    /// Construct a missing-subscription error.
    #[must_use]
    pub fn not_found(identifier: &Identifier) -> SubscriptionError {
        SubscriptionError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct an identifier-conflict error.
    #[must_use]
    pub fn already_exists(identifier: &Identifier) -> SubscriptionError {
        SubscriptionError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::SubscriptionError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(
            SubscriptionError::from(PersistenceFailure::Unavailable),
            SubscriptionError::StoreUnavailable
        ));
        assert!(matches!(
            SubscriptionError::from(PersistenceFailure::Misconfigured),
            SubscriptionError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
