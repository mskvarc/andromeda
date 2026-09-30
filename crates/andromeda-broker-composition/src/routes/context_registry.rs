//! Composition of focused Context Registry HTTP capabilities.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_http_context_registry::state::{ContextRegistryRouteServices, ContextRegistryStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves one broker tenant and projects only Context Registry capabilities.
pub struct BrokerContextRegistryStateResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerContextRegistryStateResolver {
    /// Construct a focused resolver over the tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerContextRegistryStateResolver {
        BrokerContextRegistryStateResolver { tenants }
    }
}

impl ContextRegistryStateResolver for BrokerContextRegistryStateResolver {
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<ContextRegistryRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(tenant, creation_policy).await?;
            Ok(Arc::new(ContextRegistryRouteServices::new(
                state.context_hosting.clone(), // clone: the route services own a handle on the tenant's hosting
                lease,
            )))
        })
    }
}
