//! Composition of focused snapshot HTTP capabilities.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_http_snapshot::state::{SnapshotRouteServices, SnapshotStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves one broker tenant and projects only snapshot capabilities.
pub(crate) struct BrokerSnapshotStateResolver {
    /// The process tenant runtimes; each resolution leases the tenant's state
    /// for the lifetime of the request.
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerSnapshotStateResolver {
    /// Construct a focused resolver over the tenant runtime manager.
    pub(crate) const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerSnapshotStateResolver {
        BrokerSnapshotStateResolver { tenants }
    }
}

impl SnapshotStateResolver for BrokerSnapshotStateResolver {
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<SnapshotRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(tenant, creation_policy).await?;
            Ok(Arc::new(SnapshotRouteServices::new(
                Arc::clone(&state.snapshots),
                state.entity_query.engine().clone(),
                lease,
            )))
        })
    }
}
