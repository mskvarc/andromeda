//! Why a tenant could not be served.

use crate::{runtime_build_failure::RuntimeBuildFailure, tenant_ref::TenantRef};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

/// Failures reaching or building a tenant's runtime.
///
/// A failure that came from a store or a composition root this crate cannot
/// name arrives classified, as a [`PersistenceFailure`] or a
/// [`RuntimeBuildFailure`]; the store's own detail is recorded for operators
/// where it is classified.
#[derive(Debug, thiserror::Error)]
pub enum TenancyError {
    /// A named tenant was addressed on a deployment that serves only the
    /// unnamed default.
    #[error("multi-tenant support is not enabled")]
    MultiTenancyDisabled,

    /// The tenant is not registered and the route does not create one.
    #[error("tenant '{name}' does not exist")]
    UnknownTenant {
        /// The tenant that was addressed.
        name: TenantName,
    },

    /// The tenant registry could not be read.
    #[error("the tenant registry could not be read: {0}")]
    RegistryUnavailable(#[source] PersistenceFailure),

    /// The tenant could not be brought into existence.
    #[error("tenant '{name}' could not be provisioned: {cause}")]
    ProvisioningFailed {
        /// The tenant that was being created.
        name: TenantName,
        /// How the store failed.
        #[source]
        cause: PersistenceFailure,
    },

    /// The tenant is registered but its runtime could not be built.
    #[error("the runtime for tenant {tenant} could not be built: {cause}")]
    RuntimeUnavailable {
        /// The tenant whose runtime failed.
        tenant: TenantRef,
        /// Why the composition root could not build it.
        #[source]
        cause: RuntimeBuildFailure,
    },

    /// The tenant's backend connection pool was saturated while reaching or
    /// building its runtime.
    ///
    /// This is backpressure, not a fault: the pooler or the PostgreSQL backend
    /// cap left no connection to spare for a cold lookup, provision, or build
    /// within the acquire deadline. The caller's only
    /// response is to retry with backoff, so it maps to a 503 with `Retry-After`
    /// rather than a 500.
    #[error("tenant {tenant} is temporarily saturated; retry with backoff")]
    Saturated {
        /// The tenant whose backend pool was saturated.
        tenant: TenantRef,
    },
}
