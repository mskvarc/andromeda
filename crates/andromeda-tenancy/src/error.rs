//! Why a tenant could not be served.

use crate::tenant_ref::TenantRef;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;

/// Failures reaching or building a tenant's runtime.
///
/// Three variants carry a `detail: String`. That is deliberate and is the one
/// place this crate holds unstructured text: the failure came from a store or
/// a composition root this crate cannot name, and inventing a typed
/// vocabulary for "what PostgreSQL said" here would be inventing it for every
/// backend at once. What the *caller* needs — which of the three things went
/// wrong — is the variant, not the message.
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
    #[error("the tenant registry could not be read: {detail}")]
    RegistryUnavailable {
        /// What the store reported.
        detail: String,
    },

    /// The tenant could not be brought into existence.
    #[error("tenant '{name}' could not be provisioned: {detail}")]
    ProvisioningFailed {
        /// The tenant that was being created.
        name: TenantName,
        /// What the store reported.
        detail: String,
    },

    /// The tenant is registered but its runtime could not be built.
    #[error("the runtime for tenant {tenant} could not be built: {detail}")]
    RuntimeUnavailable {
        /// The tenant whose runtime failed.
        tenant: TenantRef,
        /// What the composition root reported.
        detail: String,
    },

    /// The tenant's backend connection pool was saturated while reaching or
    /// building its runtime.
    ///
    /// This is backpressure, not a fault: the pooler or the PostgreSQL backend
    /// cap left no connection to spare for a cold lookup, provision, or build
    /// within the acquire deadline. Unlike the three fault variants it carries
    /// no opaque `detail` — the condition is fully typed, and the caller's only
    /// response is to retry with backoff, so it maps to a 503 with `Retry-After`
    /// rather than a 500.
    #[error("tenant {tenant} is temporarily saturated; retry with backoff")]
    Saturated {
        /// The tenant whose backend pool was saturated.
        tenant: TenantRef,
    },
}
