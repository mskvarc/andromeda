//! Focused tenant construction failures.

use andromeda_jsonld::error::JsonLdError;
use andromeda_postgresql_context_store::error::ContextStoreError;
use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_postgresql_temporal_store::error::TemporalStoreError;
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
    /// The temporal hypertables' storage policy could not be reconciled.
    #[error("tenant temporal storage policy reconciliation failed: {0}")]
    TemporalStoragePolicy(#[from] TemporalStoreError),
    /// A typed construction invariant could not be established.
    #[error("tenant runtime construction failed: {0}")]
    Invariant(String),
}

impl TenantAssemblyError {
    /// Preserve operator-facing detail for a failed typed construction invariant.
    pub(super) fn invariant(detail: impl Into<String>) -> TenantAssemblyError {
        TenantAssemblyError::Invariant(detail.into())
    }

    /// Whether construction failed because a backend connection pool was
    /// saturated, rather than because of a genuine fault.
    ///
    /// A saturated pool is shed as backpressure (a 503) rather than reported as
    /// an internal error: a cold build acquires connections against the serving
    /// pool, and under a tight pooler budget that acquire can time out. The
    /// timeout arrives either as a raw [`SqlxError`] the pool returned, or
    /// already funnelled into a store error's saturation category.
    pub(super) const fn is_saturation(&self) -> bool {
        match self {
            TenantAssemblyError::Database(error) => is_pool_saturated(error),
            TenantAssemblyError::ContextStore(error) => error.is_pool_saturation(),
            TenantAssemblyError::TemporalStoragePolicy(error) => matches!(error, TemporalStoreError::Database(inner) if is_pool_saturated(inner)),
            TenantAssemblyError::Migration(_) | TenantAssemblyError::JsonLd(_) | TenantAssemblyError::Invariant(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::error::TenantAssemblyError;
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use sqlx::Error as SqlxError;

    #[test]
    fn a_pool_timeout_during_provisioning_becomes_saturated() {
        // A pool-acquire timeout, raw from the pool or funnelled through the
        // context store, is saturation; a real fault is not.
        assert!(TenantAssemblyError::Database(SqlxError::PoolTimedOut).is_saturation());
        assert!(TenantAssemblyError::ContextStore(ContextStoreError::Saturated).is_saturation());
        assert!(!TenantAssemblyError::Database(SqlxError::RowNotFound).is_saturation());
        assert!(!TenantAssemblyError::invariant("not a pool problem").is_saturation());
    }
}
