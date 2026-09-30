//! Errors produced by context-source registration policy and persistence.

use crate::{
    registration_conflict_reason::RegistrationConflictReason,
    registration_invariant::RegistrationInvariant,
    registration_validation_reason::RegistrationValidationReason,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use thiserror::Error;

/// A failure while validating, applying, or persisting a registration operation.
#[derive(Debug, Error)]
pub enum RegistrationError {
    /// The request does not satisfy the registration contract.
    #[error("invalid context-source registration: {0}")]
    Validation(#[from] RegistrationValidationReason),
    /// The requested registration does not exist.
    #[error("context-source registration not found: {identifier}")]
    NotFound {
        /// Missing registration identifier.
        identifier: Identifier,
    },
    /// A registration already uses the requested identifier.
    #[error("context-source registration already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting registration identifier.
        identifier: Identifier,
    },
    /// A proxied registration overlaps an existing registration or a local
    /// entity (clauses 5.9.2.4 and 5.9.3.4, `Conflict`).
    #[error("context-source registration conflicts with existing context data: {0}")]
    Conflict(RegistrationConflictReason),
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// Query compilation failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// Registration identifier generation failed.
    #[error(transparent)]
    Identifier(#[from] urn_rs::Error),
    /// The persistence adapter could not complete the requested operation.
    #[error("registration persistence failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// An internal registration invariant could not be satisfied.
    #[error("registration invariant failed: {0}")]
    Internal(#[from] RegistrationInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store is backpressure the client can retry, so it keeps its own
/// retryable category; every other failure is a persistence fault.
impl From<PersistenceFailure> for RegistrationError {
    fn from(failure: PersistenceFailure) -> RegistrationError {
        match failure {
            PersistenceFailure::Saturated => RegistrationError::StoreSaturated,
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
            | PersistenceFailure::ContractBroken) => RegistrationError::Persistence(fault),
        }
    }
}

impl RegistrationError {
    /// Construct a missing-registration error.
    #[must_use]
    pub fn not_found(identifier: &Identifier) -> RegistrationError {
        RegistrationError::NotFound {
            identifier: identifier.clone(), // clone: the error must own the borrowed request identifier.
        }
    }

    /// Construct an identifier-conflict error.
    #[must_use]
    pub fn already_exists(identifier: &Identifier) -> RegistrationError {
        RegistrationError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error must outlive the borrowed request identifier.
        }
    }
}
