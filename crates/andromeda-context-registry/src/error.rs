//! Failures reported by Context Registry policy and implementations.

use crate::{identifier::ContextIdentifier, registry_invariant::RegistryInvariant};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use iri_rs::IriBuf;
use thiserror::Error;

/// A hosted-context semantic failure or backend-neutral implementation failure.
#[derive(Debug, Error)]
pub enum ContextRegistryError {
    /// A hosted Context payload omitted its required top-level `@context` member.
    #[error("payload must contain a top-level @context field")]
    MissingTopLevelContext,
    /// The requested Context is not registered.
    #[error("context not found: {identifier}")]
    NotFound {
        /// The identifier the request wrote, whether or not it keys a row.
        identifier: ContextIdentifier,
    },
    /// An embedded core Context cannot be deleted.
    #[error("an embedded NGSI-LD core context cannot be deleted")]
    CoreContextDeletion,
    /// The full content of a `Cached` Context was requested.
    ///
    /// Clause 5.13.4.4: a served `@context` comes from internal storage only
    /// when it is of kind `Hosted` or `ImplicitlyCreated`; a `Cached` one is a
    /// local performance copy of someone else's document (clause 5.13.1) and
    /// serving it would republish it under this broker's identifier.
    #[error("a cached context is not served on demand: {identifier}")]
    CachedContextNotServed {
        /// The cached Context identifier.
        identifier: Identifier,
    },
    /// Reload was requested for a Context that was not populated by the cache.
    #[error("only cached contexts can be reloaded: {identifier}")]
    ReloadRequiresCached {
        /// The non-cached Context identifier.
        identifier: Identifier,
    },
    /// A cached Context row has no remote source URL.
    #[error("cached context has no source URL: {identifier}")]
    MissingSourceUrl {
        /// The malformed cached Context identifier.
        identifier: Identifier,
    },
    /// A remote Context could not be downloaded for reload.
    #[error("context is not available from {source_url}: {source}")]
    ContextUnavailable {
        /// The remote Context source.
        source_url: IriBuf,
        /// Why the resolver could not provide the document.
        #[source]
        source: Box<JsonLdError>,
    },
    /// A persistence implementation could not complete an operation.
    #[error("context registry persistence failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// A response could not be represented as JSON.
    #[error("context registry representation failed: {0}")]
    Representation(#[source] serde_json::Error),
    /// Policy produced an invalid domain value.
    #[error("context registry invariant failed: {0}")]
    Invariant(#[from] RegistryInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store is backpressure the client can retry, so it keeps its own
/// retryable category; every other failure is a persistence fault.
impl From<PersistenceFailure> for ContextRegistryError {
    fn from(failure: PersistenceFailure) -> ContextRegistryError {
        match failure {
            PersistenceFailure::Saturated => ContextRegistryError::StoreSaturated,
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
            | PersistenceFailure::ContractBroken) => ContextRegistryError::Persistence(fault),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::ContextRegistryError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    #[test]
    fn persistence_error_does_not_expose_a_backend_type() {
        let error = ContextRegistryError::from(PersistenceFailure::Unavailable);
        pretty_assertions::assert_eq!(error.to_string(), "context registry persistence failed: the store is unreachable");
    }

    #[test]
    fn a_saturated_store_keeps_its_retryable_category() {
        assert!(matches!(
            ContextRegistryError::from(PersistenceFailure::Saturated),
            ContextRegistryError::StoreSaturated
        ));
    }
}
