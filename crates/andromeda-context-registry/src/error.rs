//! Failures reported by Context Registry policy and implementations.

use crate::identifier::ContextIdentifier;
use andromeda_ngsi_ld_lexicon::uri::Uri;
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
        identifier: Uri,
    },
    /// Reload was requested for a Context that was not populated by the cache.
    #[error("only cached contexts can be reloaded: {identifier}")]
    ReloadRequiresCached {
        /// The non-cached Context identifier.
        identifier: Uri,
    },
    /// A cached Context row has no remote source URL.
    #[error("cached context has no source URL: {identifier}")]
    MissingSourceUrl {
        /// The malformed cached Context identifier.
        identifier: Uri,
    },
    /// A remote Context could not be downloaded for reload.
    #[error("context is not available from {source_url}: {detail}")]
    ContextUnavailable {
        /// The remote Context source.
        source_url: IriBuf,
        /// Human-readable resolver failure detail.
        detail: String,
    },
    /// A persistence implementation could not complete an operation.
    #[error("context registry persistence failed: {detail}")]
    Persistence {
        /// Human-readable backend-neutral failure detail.
        detail: String,
    },
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// A response could not be represented as JSON.
    #[error("context registry representation failed: {detail}")]
    Representation {
        /// Human-readable serialization failure detail.
        detail: String,
    },
    /// Policy or an implementation produced an invalid domain value.
    #[error("context registry invariant failed: {detail}")]
    Invariant {
        /// Human-readable invariant failure detail.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::error::ContextRegistryError;

    #[test]
    fn persistence_error_does_not_expose_a_backend_type() {
        let error = ContextRegistryError::Persistence {
            detail: "connection closed".to_string(),
        };
        pretty_assertions::assert_eq!(error.to_string(), "context registry persistence failed: connection closed");
    }
}
