//! Composition of focused Entity Map HTTP capabilities.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_http_entity_map::state::{EntityMapRouteServices, EntityMapStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves one broker tenant and projects only Entity Map capabilities.
pub struct BrokerEntityMapStateResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerEntityMapStateResolver {
    /// Construct a focused resolver over the tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerEntityMapStateResolver {
        BrokerEntityMapStateResolver { tenants }
    }
}

impl EntityMapStateResolver for BrokerEntityMapStateResolver {
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<EntityMapRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(tenant, creation_policy).await?;
            Ok(Arc::new(EntityMapRouteServices::new(
                Arc::clone(&state.entity_maps),
                Arc::clone(&state.entity_query),
                Arc::clone(&state.temporal),
                Arc::clone(&state.forwarding),
                state.live_target.clone(),
                lease,
            )))
        })
    }
}
