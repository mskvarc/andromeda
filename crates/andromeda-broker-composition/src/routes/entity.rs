//! Composition adapters that supply focused broker capabilities to entity HTTP routes.

use crate::{routes::context_target::BrokerSnapshotTargetResolver, tenant_factory::BrokerTenantFactory};
use andromeda_http_entity::state::{EntityRouteServices, EntityStateResolver};
use andromeda_http_extraction::context_target::SnapshotTargetResolver;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves a complete broker tenant and projects only entity-route capabilities from it.
pub(crate) struct BrokerEntityStateResolver {
    /// The process tenant runtimes; each resolution leases the tenant's state
    /// for the lifetime of the request.
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerEntityStateResolver {
    /// Construct the focused resolver over the process tenant runtime manager.
    pub(crate) const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerEntityStateResolver {
        BrokerEntityStateResolver { tenants }
    }
}

impl EntityStateResolver for BrokerEntityStateResolver {
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<EntityRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(tenant, creation_policy).await?;
            let snapshot_targets: Arc<dyn SnapshotTargetResolver> = Arc::new(BrokerSnapshotTargetResolver::new(Arc::clone(&state.snapshots)));
            Ok(Arc::new(
                EntityRouteServices::builder()
                    .runtime_lease(lease)
                    .entities(Arc::clone(&state.entities))
                    .entity_query(Arc::clone(&state.entity_query))
                    .entity_discovery(Arc::clone(&state.entity_discovery))
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
