//! Composition of focused subscription HTTP capabilities.

use crate::api::{state::AndromedaState, tenant_factory::BrokerTenantFactory};
use andromeda_http_subscription::state::{SubscriptionRouteServices, SubscriptionStateResolver};
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves one broker tenant and projects only subscription capabilities.
pub struct BrokerSubscriptionStateResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerSubscriptionStateResolver {
    /// Construct a focused resolver over the tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerSubscriptionStateResolver {
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
            let state: Arc<AndromedaState> = self.tenants.resolve_state(tenant, creation_policy).await?;
            Ok(Arc::new(
                SubscriptionRouteServices::builder()
                    .subscriptions(Arc::clone(&state.subscriptions))
                    .csource_subscriptions(Arc::clone(&state.csr_subscriptions))
                    .sse_registry(Arc::clone(&state.sse_registry))
                    .sse_channel_buffer_size(state.sse_channel_buffer_size)
                    .sse_keep_alive_interval_secs(state.sse_keep_alive_interval_secs)
                    .ws_registry(Arc::clone(&state.ws_registry))
                    .ws_channel_buffer_size(state.ws_channel_buffer_size)
                    .ws_ping_interval_secs(state.ws_ping_interval_secs)
                    .build(),
            ))
        })
    }
}
