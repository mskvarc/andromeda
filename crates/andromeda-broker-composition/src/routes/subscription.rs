//! Composition of focused subscription HTTP capabilities.

use crate::{routes::context_target::BrokerSnapshotTargetResolver, tenant_factory::BrokerTenantFactory};
use andromeda_http_extraction::context_target::SnapshotTargetResolver;
use andromeda_http_subscription::state::{SubscriptionRouteServices, SubscriptionStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves one broker tenant and projects only subscription capabilities.
pub(crate) struct BrokerSubscriptionStateResolver {
    /// The process tenant runtimes; each resolution leases the tenant's state
    /// for the lifetime of the request.
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerSubscriptionStateResolver {
    /// Construct a focused resolver over the tenant runtime manager.
    pub(crate) const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerSubscriptionStateResolver {
        BrokerSubscriptionStateResolver { tenants }
    }
}

impl SubscriptionStateResolver for BrokerSubscriptionStateResolver {
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<SubscriptionRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(tenant, creation_policy).await?;
            let snapshot_targets: Arc<dyn SnapshotTargetResolver> = Arc::new(BrokerSnapshotTargetResolver::new(Arc::clone(&state.snapshots)));
            Ok(Arc::new(
                SubscriptionRouteServices::builder()
                    .runtime_lease(lease)
                    .live_target(state.live_target.clone())
                    .snapshot_targets(snapshot_targets)
                    .subscriptions(Arc::clone(&state.subscriptions))
                    .csource_subscriptions(Arc::clone(&state.csr_subscriptions))
                    .engine(state.entity_query.engine().clone())
                    .context_hosting(state.context_hosting.clone())
                    .build(),
            ))
        })
    }
}
