//! Focused tenant construction failures.

use crate::tenant_factory::assembly_invariant::AssemblyInvariant;
use andromeda_jsonld::error::JsonLdError;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_context_store::error::ContextStoreError;
use andromeda_postgresql_pool::{persistence_classification::classify, saturation::is_pool_saturated};
use andromeda_postgresql_temporal_store::error::TemporalStoreError;
use andromeda_tenancy::{error::TenancyError, runtime_build_failure::RuntimeBuildFailure, saturated_wait::SaturatedWait, tenant_ref::TenantRef};
use sqlx::Error as SqlxError;
use thiserror::Error;

/// Failure while constructing one tenant's complete runtime.
#[derive(Debug, Error)]
pub(super) enum TenantAssemblyError {
    /// A PostgreSQL operation failed during construction.
    #[error("tenant database construction failed: {0}")]
    Database(#[from] SqlxError),
    /// A context-store adapter failed during construction.
    #[error("tenant context storage construction failed: {0}")]
    ContextStore(#[from] ContextStoreError),
    /// JSON-LD engine construction failed.
    #[error("tenant JSON-LD construction failed: {0}")]
    JsonLd(#[from] JsonLdError),
    /// The temporal hypertables' storage policy could not be reconciled.
    #[error("tenant temporal storage policy reconciliation failed: {0}")]
    TemporalStoragePolicy(#[from] TemporalStoreError),
    /// A part of the runtime refused to be constructed.
    #[error("tenant runtime construction failed: {0}")]
    Invariant(#[from] AssemblyInvariant),
}

impl TenantAssemblyError {
    /// The backend-neutral cause a tenancy port reports this failure as.
    pub(super) fn build_failure(&self) -> RuntimeBuildFailure {
        match self {
            TenantAssemblyError::Database(error) => RuntimeBuildFailure::Persistence(classify(error)),
            TenantAssemblyError::ContextStore(error) => RuntimeBuildFailure::Persistence(error.failure_class()),
            TenantAssemblyError::TemporalStoragePolicy(error) => {
                RuntimeBuildFailure::Persistence(error.failure_class().unwrap_or(PersistenceFailure::ContractBroken))
            }
            TenantAssemblyError::JsonLd(_) => RuntimeBuildFailure::JsonLdEngine,
            TenantAssemblyError::Invariant(_) => RuntimeBuildFailure::Invariant,
        }
    }

    /// Whether construction failed because a backend connection pool was
    /// saturated, rather than because of a genuine fault.
    ///
    /// A saturated pool is shed as backpressure (a 503) rather than reported as
    /// an internal error: a cold build acquires connections against the serving
    /// pool, and under a tight pooler budget that acquire can time out. The
    /// timeout arrives either as a raw [`SqlxError`] the pool returned, or
    /// already funnelled into a store error's saturation category.
    pub(super) fn is_saturation(&self) -> bool {
        match self {
            TenantAssemblyError::Database(error) => is_pool_saturated(error),
            TenantAssemblyError::ContextStore(error) => error.is_pool_saturation(),
            TenantAssemblyError::TemporalStoragePolicy(error) => matches!(error, TemporalStoreError::Database(inner) if is_pool_saturated(inner)),
            TenantAssemblyError::JsonLd(_) | TenantAssemblyError::Invariant(_) => false,
        }
    }

    /// The refusal the tenancy port reports for a runtime `tenant` could not
    /// be built.
    ///
    /// A saturated backend pool is backpressure, shed as a 503 naming the
    /// runtime build's pool; only a genuine construction fault is the internal
    /// [`TenancyError::RuntimeUnavailable`], and it is logged here with the
    /// detail the port does not carry.
    pub(super) fn into_tenancy_error(self, tenant: &TenantRef) -> TenancyError {
        if self.is_saturation() {
            return TenancyError::Saturated {
                tenant: tenant.clone(), // clone: the error outlives this borrow
                wait: SaturatedWait::RuntimePool,
            };
        }
        tracing::error!(%tenant, error = %self, "the tenant runtime could not be built");
        TenancyError::RuntimeUnavailable {
            tenant: tenant.clone(), // clone: the error outlives this borrow
            cause: self.build_failure(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::{
        assembly_invariant::{AssemblyInvariant, RetentionSetting},
        error::TenantAssemblyError,
    };
    use andromeda_ngsi_ld_lexicon::tenant::{InvalidTenantName, TenantName};
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use andromeda_tenancy::{error::TenancyError, runtime_build_failure::RuntimeBuildFailure, saturated_wait::SaturatedWait, tenant_ref::TenantRef};
    use sqlx::Error as SqlxError;

    #[test]
    fn a_pool_timeout_during_provisioning_becomes_saturated() {
        // A pool-acquire timeout, raw from the pool or funnelled through the
        // context store, is saturation; a real fault is not.
        assert!(TenantAssemblyError::Database(SqlxError::PoolTimedOut).is_saturation());
        assert!(TenantAssemblyError::ContextStore(ContextStoreError::Saturated).is_saturation());
        assert!(!TenantAssemblyError::Database(SqlxError::RowNotFound).is_saturation());
        assert!(!TenantAssemblyError::from(AssemblyInvariant::RetentionOutOfRange(RetentionSetting::SideEffectDeadLetter)).is_saturation());
    }

    #[test]
    fn a_saturated_runtime_build_names_the_runtime_pool() -> Result<(), InvalidTenantName> {
        let tenant = TenantRef::Named(TenantName::try_from("acme".to_owned())?);
        for error in [
            TenantAssemblyError::Database(SqlxError::PoolTimedOut),
            TenantAssemblyError::ContextStore(ContextStoreError::Saturated),
        ] {
            let refused = error.into_tenancy_error(&tenant);
            assert!(
                matches!(
                    refused,
                    TenancyError::Saturated {
                        wait: SaturatedWait::RuntimePool,
                        ..
                    }
                ),
                "{refused:?}"
            );
        }
        let fault = TenantAssemblyError::from(AssemblyInvariant::RetentionOutOfRange(RetentionSetting::SideEffectDeadLetter)).into_tenancy_error(&tenant);
        assert!(
            matches!(
                fault,
                TenancyError::RuntimeUnavailable {
                    cause: RuntimeBuildFailure::Invariant,
                    ..
                }
            ),
            "{fault:?}"
        );
        Ok(())
    }
}
