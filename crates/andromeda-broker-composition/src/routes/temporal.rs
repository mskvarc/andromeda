//! Composition adapters that supply focused broker capabilities to temporal HTTP routes.

use crate::{routes::context_target::BrokerSnapshotTargetResolver, tenant_factory::BrokerTenantFactory};
use andromeda_http_extraction::context_target::SnapshotTargetResolver;
use andromeda_http_temporal::state::{TemporalRouteServices, TemporalStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves a complete broker tenant and projects only temporal-route capabilities from it.
pub(crate) struct BrokerTemporalStateResolver {
    /// The process tenant runtimes; each resolution leases the tenant's state
    /// for the lifetime of the request.
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerTemporalStateResolver {
    /// Construct the focused resolver over the process tenant runtime manager.
    pub(crate) const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerTemporalStateResolver {
        BrokerTemporalStateResolver { tenants }
    }
}

impl TemporalStateResolver for BrokerTemporalStateResolver {
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<TemporalRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(tenant, creation_policy).await?;
            let snapshot_targets: Arc<dyn SnapshotTargetResolver> = Arc::new(BrokerSnapshotTargetResolver::new(Arc::clone(&state.snapshots)));
            Ok(Arc::new(
                TemporalRouteServices::builder()
                    .runtime_lease(lease)
                    .temporal(Arc::clone(&state.temporal))
                    .forwarding(Arc::clone(&state.forwarding))
                    .entity_maps(Arc::clone(&state.entity_maps))
                    .live_target(state.live_target.clone())
                    .snapshot_targets(snapshot_targets)
                    .context_hosting(state.context_hosting.clone()) // clone: the route services own a handle on the tenant's hosting
                    .build(),
            ))
        })
    }
}
