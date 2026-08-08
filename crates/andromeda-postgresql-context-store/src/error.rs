//! Error types for the context store crate.

use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_query::{attribute_name_lookup::AttributeNameLookupError, error::RenderError};

/// Errors from the PostgreSQL context store.
///
/// Variants that carry the PG error code directly (`UniqueViolation`,
/// `InvalidRegex`) separate the two codes the broker's policy layer
/// currently inspects from the generic `Database` bucket, so the
/// `From<ContextStoreError>` impl in the binary can map them to the
/// correct HTTP status without re-inspecting the driver error.
#[derive(Debug, thiserror::Error)]
pub enum ContextStoreError {
    /// A database operation failed.
    #[error("context store: {0}")]
    Database(sqlx::Error),
    /// EWKB geometry encoding failed.
    #[error("context store encoding: {0}")]
    Encoding(#[from] EwkbError),
    /// A `23505` unique-violation raised by the database.
    #[error("context store unique violation: {0}")]
    UniqueViolation(sqlx::Error),
    /// A `2201B` invalid-regular-expression raised by the database.
    #[error("context store regex: {0}")]
    InvalidRegex(sqlx::Error),
    /// A SQL statement could not be rendered from the compiled query.
    #[error("context store render: {0}")]
    Render(#[from] RenderError),
    /// An attribute-name dictionary lookup failed.
    #[error("context store attribute lookup: {0}")]
    AttributeLookup(#[from] AttributeNameLookupError),
    /// A database migration failed.
    #[error("context store migration: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    /// An internal invariant was violated (malformed stored data, serialization
    /// failure, etc.).
    #[error("context store internal: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for ContextStoreError {
    fn from(error: sqlx::Error) -> ContextStoreError {
        if let sqlx::Error::Database(ref db_error) = error {
            match db_error.code().as_deref() {
                Some("23505") => return ContextStoreError::UniqueViolation(error),
                Some("2201B") => return ContextStoreError::InvalidRegex(error),
                _ => {}
            }
        }
        ContextStoreError::Database(error)
    }
}
