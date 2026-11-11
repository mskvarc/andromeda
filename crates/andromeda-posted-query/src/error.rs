//! Failures reported by posted-query policy and storage implementations.

use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
use andromeda_persistence_failure::{persistence_failure::PersistenceFailure, retryability::RetryCause};
use thiserror::Error;

/// A semantic posted-query failure or an implementation-neutral storage
/// failure.
#[derive(Debug, Error)]
pub enum PostedQueryError {
    /// No live posted query of the API asked for carries the key: none was
    /// held under it, or the query expired or was evicted.
    #[error("no kept query is named {}", .key.encode())]
    NotFound {
        /// The key that named no live query.
        key: PageQuery,
    },
    /// A request following a pointer of a posted query names another
    /// `@context` than the query was posted with (clause 5.5.9.1).
    #[error("the request names another @context than the posted query it follows was read under")]
    ContextChanged,
    /// The query cannot be written as the JSON its key is digested from.
    #[error("the posted query cannot be written as JSON: {0}")]
    Unencodable(#[source] serde_json::Error),
    /// The configured lifetime, counted from now, passes the representable
    /// time range.
    #[error("the posted query lifetime is outside the representable time range")]
    LifetimeOutOfRange,
    /// A storage implementation could not complete an operation.
    #[error("posted query storage failed: {0}")]
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
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a store that cannot be reached are conditions the
/// client can retry, so each keeps its own retryable category; every other
/// failure is a storage fault.
impl From<PersistenceFailure> for PostedQueryError {
    fn from(failure: PersistenceFailure) -> PostedQueryError {
        match failure {
            PersistenceFailure::Saturated => PostedQueryError::StoreSaturated,
            PersistenceFailure::Unavailable => PostedQueryError::StoreUnavailable,
            fault @ (PersistenceFailure::RolledBack
            | PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => PostedQueryError::Persistence(fault),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::PostedQueryError;
    use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_missing_query_error_names_the_key_as_a_link_spells_it() {
        let key = PageQuery::of(Some("type=Vehicle"));
        let token = key.encode();
        assert_eq!(PostedQueryError::NotFound { key }.to_string(), format!("no kept query is named {token}"));
    }

    #[test]
    fn a_saturated_or_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(
            PostedQueryError::from(PersistenceFailure::Saturated),
            PostedQueryError::StoreSaturated
        ));
        assert!(matches!(
            PostedQueryError::from(PersistenceFailure::Unavailable),
            PostedQueryError::StoreUnavailable
        ));
        assert!(matches!(
            PostedQueryError::from(PersistenceFailure::Undecodable),
            PostedQueryError::Persistence(PersistenceFailure::Undecodable)
        ));
    }
}
