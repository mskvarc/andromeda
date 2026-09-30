//! Errors produced by subscription policy and persistence.

use crate::{subscription_invariant::SubscriptionInvariant, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_jsonld::error::JsonLdError;
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
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// Query parsing or expansion failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// Subscription identifier generation failed.
    #[error(transparent)]
    Identifier(#[from] urn_rs::Error),
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
    /// An internal subscription invariant could not be satisfied.
    #[error("subscription invariant failed: {0}")]
    Internal(#[from] SubscriptionInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store is backpressure the client can retry, so it keeps its own
/// retryable category; every other failure is a persistence fault.
impl From<PersistenceFailure> for SubscriptionError {
    fn from(failure: PersistenceFailure) -> SubscriptionError {
        match failure {
            PersistenceFailure::Saturated => SubscriptionError::StoreSaturated,
            fault @ (PersistenceFailure::RolledBack
            | PersistenceFailure::Unavailable
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
