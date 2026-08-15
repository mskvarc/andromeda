//! Focused tenant-state resolution for subscription routes.

use andromeda_delivery::stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage};
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    tenant_state::{ExistingTenantState, ProvisionableTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_subscription::{csource::CsrSubscriptionVariant, entity::EntitySubscriptionVariant, lifecycle::SubscriptionService};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The complete subscription capability set exposed by one tenant.
#[derive(TypedBuilder)]
pub struct SubscriptionRouteServices {
    /// Entity-subscription lifecycle policy.
    pub subscriptions: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    /// Context-source-subscription lifecycle policy.
    pub csource_subscriptions: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    /// Live SSE connections.
    pub sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
    /// Per-connection SSE channel capacity.
    pub sse_channel_buffer_size: usize,
    /// SSE keep-alive cadence in seconds.
    pub sse_keep_alive_interval_secs: u64,
    /// Live WebSocket connections.
    pub ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>>,
    /// Per-connection WebSocket channel capacity.
    pub ws_channel_buffer_size: usize,
    /// WebSocket ping cadence in seconds.
    pub ws_ping_interval_secs: u64,
}

/// Existing-tenant extraction specialized to subscription capabilities.
pub(crate) type SubscriptionTenantState = ExistingTenantState<Arc<SubscriptionRouteServices>>;
/// Provisionable-tenant extraction specialized to subscription capabilities.
pub(crate) type SubscriptionTenantStateOrCreate = ProvisionableTenantState<Arc<SubscriptionRouteServices>>;

/// Resolves only subscription capabilities for one tenant scope.
pub trait SubscriptionStateResolver: Send + Sync {
    /// Resolve or create the tenant according to the supplied policy.
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<SubscriptionRouteServices>, TenancyError>>;
}

/// Axum state for the complete subscription route fragment.
#[derive(Clone)]
pub(crate) struct SubscriptionRouteState {
    resolver: Arc<dyn SubscriptionStateResolver>,
}

impl SubscriptionRouteState {
    pub(crate) const fn new(resolver: Arc<dyn SubscriptionStateResolver>) -> SubscriptionRouteState {
        SubscriptionRouteState { resolver }
    }
}

impl TenantStateResolver<Arc<SubscriptionRouteServices>> for SubscriptionRouteState {
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<Arc<SubscriptionRouteServices>, ProblemResponse>> {
        Box::pin(async move {
            self.resolver
                .resolve(tenant, creation_policy)
                .await
                .map_err(|error| classify_tenancy_error(&error).with_instance(instance.clone()))
        })
    }
}
