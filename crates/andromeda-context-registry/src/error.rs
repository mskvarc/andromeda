//! Failures reported by Context Registry policy and implementations.

use crate::{identifier::ContextIdentifier, registry_invariant::RegistryInvariant};
use andromeda_jsonld::{context_subtree::UnusableLocalContext, error::JsonLdError};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use iri_rs::IriBuf;
use thiserror::Error;

/// A hosted-context semantic failure or backend-neutral implementation failure.
#[derive(Debug, Error)]
pub enum ContextRegistryError {
    /// A hosted Context payload is valid JSON but carries no local context: it
    /// is not an object, has no top-level `@context` member, or that member
    /// holds no JSON-LD context (clause 5.13.2.3).
    #[error("payload must carry a JSON-LD context in a top-level @context member: {0}")]
    NoLocalContext(#[source] UnusableLocalContext),
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
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore is temporarily unavailable; retry shortly")]
    StoreUnavailable,
    /// Policy produced an invalid domain value.
    #[error("context registry invariant failed: {0}")]
    Invariant(#[from] RegistryInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a store that cannot be reached are conditions the
/// client can retry, so each keeps its own retryable category; every other
/// failure is a persistence fault.
impl From<PersistenceFailure> for ContextRegistryError {
    fn from(failure: PersistenceFailure) -> ContextRegistryError {
        match failure {
            PersistenceFailure::Saturated => ContextRegistryError::StoreSaturated,
            PersistenceFailure::Unavailable => ContextRegistryError::StoreUnavailable,
            fault @ (PersistenceFailure::RolledBack
            | PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
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
        let error = ContextRegistryError::from(PersistenceFailure::Undecodable);
        pretty_assertions::assert_eq!(error.to_string(), "context registry persistence failed: a stored value could not be decoded");
    }

    #[test]
    fn a_saturated_store_keeps_its_retryable_category() {
        assert!(matches!(
            ContextRegistryError::from(PersistenceFailure::Saturated),
            ContextRegistryError::StoreSaturated
        ));
    }

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(
            ContextRegistryError::from(PersistenceFailure::Unavailable),
            ContextRegistryError::StoreUnavailable
        ));
        assert!(matches!(
            ContextRegistryError::from(PersistenceFailure::Misconfigured),
            ContextRegistryError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
