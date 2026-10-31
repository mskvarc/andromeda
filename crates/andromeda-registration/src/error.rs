//! Errors produced by context-source registration policy and persistence.

use crate::{
    csr_member::CsrMember,
    registration_conflict_reason::RegistrationConflictReason,
    registration_invariant::RegistrationInvariant,
    registration_validation_reason::RegistrationValidationReason,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_geometry::geometry_census::GeometryOverLimit;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_persistence_failure::{persistence_failure::PersistenceFailure, retryability::RetryCause};
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
    /// A geometry the registration writes holds more positions, or more JSON
    /// values besides them, than the broker accepts in one geometry (clause
    /// 6.3.2, "Request Entity too large").
    #[error("the registration's {member} geometry holds {over}; send a simplified geometry")]
    GeometryTooLarge {
        /// The member holding the geometry.
        member: CsrMember,
        /// The limit the geometry holds more than.
        over: GeometryOverLimit,
    },
    /// Query compilation failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// The persistence adapter could not complete the requested operation.
    #[error("registration persistence failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("{}", RetryCause::Saturated)]
    StoreSaturated,
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("{}", RetryCause::Unreachable)]
    StoreUnavailable,
    /// An internal registration invariant could not be satisfied.
    #[error("registration invariant failed: {0}")]
    Internal(#[from] RegistrationInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a store that cannot be reached are conditions the
/// client can retry, so each keeps its own retryable category; every other
/// failure is a persistence fault.
impl From<PersistenceFailure> for RegistrationError {
    fn from(failure: PersistenceFailure) -> RegistrationError {
        match failure {
            PersistenceFailure::Saturated => RegistrationError::StoreSaturated,
            PersistenceFailure::Unavailable => RegistrationError::StoreUnavailable,
            fault @ (PersistenceFailure::RolledBack
            | PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
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

#[cfg(test)]
mod tests {
    use crate::error::RegistrationError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(
            RegistrationError::from(PersistenceFailure::Unavailable),
            RegistrationError::StoreUnavailable
        ));
        assert!(matches!(
            RegistrationError::from(PersistenceFailure::Misconfigured),
            RegistrationError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
