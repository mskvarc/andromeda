//! Error types for the temporal store crate.

use andromeda_postgresql_encoding::ewkb::EwkbError;

/// Errors produced by a temporal store implementation.
///
/// Internal to the temporal store boundary: the service layer converts these
/// into `AndromedaError` (via the `From` impl) rather than propagating
/// store-specific detail to API clients.
#[derive(Debug, thiserror::Error)]
pub enum TemporalStoreError {
    /// The underlying database query failed.
    #[error("temporal store: {0}")]
    Database(#[from] sqlx::Error),
    /// A stored value could not be decoded into a domain type.
    #[error("temporal store decode: {0}")]
    Decode(String),
    /// The store could not build or run the requested query.
    #[error("temporal store query: {0}")]
    Query(String),
    /// The requested entity or attribute instance was not found.
    #[error("temporal store not found: {0}")]
    NotFound(String),
    /// A geometry value could not be encoded for persistence.
    #[error("temporal store encoding: {0}")]
    Encoding(#[from] EwkbError),
    /// An attribute name could not be resolved in the dictionary.
    #[error("temporal store attribute lookup: {0}")]
    AttributeLookup(String),
    /// An extracted attribute value was invalid.
    #[error("temporal store validation: {0}")]
    Validation(String),
}
