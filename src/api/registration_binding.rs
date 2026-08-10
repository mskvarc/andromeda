//! Composition of focused registration HTTP capabilities.

use crate::api::{state::AndromedaState, tenant_factory::BrokerTenantFactory};
use andromeda_http_registration::state::{RegistrationRouteServices, RegistrationStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves one broker tenant and projects only registration capabilities.
pub struct BrokerRegistrationStateResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerRegistrationStateResolver {
    /// Construct a focused resolver over the tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerRegistrationStateResolver {
        BrokerRegistrationStateResolver { tenants }
    }
}

impl RegistrationStateResolver for BrokerRegistrationStateResolver {
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<RegistrationRouteServices>, TenancyError>> {
        Box::pin(async move {
            let state: Arc<AndromedaState> = self.tenants.resolve_state(tenant, creation_policy).await?;
            Ok(Arc::new(RegistrationRouteServices::new(Arc::clone(&state.csr))))
        })
    }
}
