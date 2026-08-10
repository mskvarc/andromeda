//! Focused tenant construction failures.

use andromeda_jsonld::error::JsonLdError;
use andromeda_postgresql_context_store::error::ContextStoreError;
use sqlx::{Error as SqlxError, migrate::MigrateError};
use thiserror::Error;

/// Failure while constructing one tenant's complete runtime.
#[derive(Debug, Error)]
pub(super) enum TenantAssemblyError {
    /// A PostgreSQL operation failed during construction.
    #[error("tenant database construction failed: {0}")]
    Database(#[from] SqlxError),
    /// A tenant schema migration failed.
    #[error("tenant database migration failed: {0}")]
    Migration(#[from] MigrateError),
    /// A context-store adapter failed during construction.
    #[error("tenant context storage construction failed: {0}")]
    ContextStore(#[from] ContextStoreError),
    /// JSON-LD engine construction failed.
    #[error("tenant JSON-LD construction failed: {0}")]
    JsonLd(#[from] JsonLdError),
    /// A typed construction invariant could not be established.
    #[error("tenant runtime construction failed: {0}")]
    Invariant(String),
}

impl TenantAssemblyError {
    /// Preserve operator-facing detail for a failed typed construction invariant.
    pub(super) fn invariant(detail: impl Into<String>) -> TenantAssemblyError {
        TenantAssemblyError::Invariant(detail.into())
    }
}
