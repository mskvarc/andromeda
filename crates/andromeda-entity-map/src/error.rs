//! Failures reported by Entity Map policy and storage implementations.

use crate::entity_map_invariant::EntityMapInvariant;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use thiserror::Error;

/// A semantic Entity Map failure or an implementation-neutral storage failure.
#[derive(Debug, Error)]
pub enum EntityMapError {
    /// The requested Entity Map is absent or has expired.
    #[error("entity map {identifier} not found")]
    NotFound {
        /// The requested Entity Map identifier.
        identifier: Identifier,
    },
    /// A follow-up request under a live map changed a filter parameter of
    /// the request that created it, which clause 5.5.14 forbids.
    #[error("the request under entity map {identifier} does not use the parameters of the request that created it")]
    RequestChanged {
        /// The Entity Map the request referenced.
        identifier: Identifier,
    },
    /// A storage implementation could not complete an operation.
    #[error("entity map storage failed: {0}")]
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
    /// Policy generated a value that violated a domain invariant.
    #[error("entity map invariant failed: {0}")]
    Invariant(#[from] EntityMapInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a store that cannot be reached are conditions the
/// client can retry, so each keeps its own retryable category; every other
/// failure is a storage fault.
impl From<PersistenceFailure> for EntityMapError {
    fn from(failure: PersistenceFailure) -> EntityMapError {
        match failure {
            PersistenceFailure::Saturated => EntityMapError::StoreSaturated,
            PersistenceFailure::Unavailable => EntityMapError::StoreUnavailable,
            fault @ (PersistenceFailure::RolledBack
            | PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => EntityMapError::Persistence(fault),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::EntityMapError;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    #[test]
    fn missing_map_error_names_its_identifier() {
        let identifier = "urn:ngsi-ld:EntityMap:missing".parse::<Identifier>().expect("a valid identifier");
        let error = EntityMapError::NotFound { identifier };
        pretty_assertions::assert_eq!(error.to_string(), "entity map urn:ngsi-ld:EntityMap:missing not found");
    }

    #[test]
    fn a_saturated_store_keeps_its_retryable_category() {
        assert!(matches!(EntityMapError::from(PersistenceFailure::Saturated), EntityMapError::StoreSaturated));
        assert!(matches!(
            EntityMapError::from(PersistenceFailure::Undecodable),
            EntityMapError::Persistence(PersistenceFailure::Undecodable)
        ));
    }

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(
            EntityMapError::from(PersistenceFailure::Unavailable),
            EntityMapError::StoreUnavailable
        ));
        assert!(matches!(
            EntityMapError::from(PersistenceFailure::Misconfigured),
            EntityMapError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
