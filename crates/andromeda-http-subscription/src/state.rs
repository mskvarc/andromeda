//! Focused tenant-state resolution for subscription routes.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::{
    classification::classify_tenancy_error,
    context_target::{ContextTargetProvider, ExistingTargetedTenantState, ProvisionableTargetedTenantState, SnapshotTargetResolver},
    tenant_state::{ExistingTenantState, ProvisionableTenantState, TenantStateResolver},
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_subscription::{csource::CsrSubscriptionVariant, entity::EntitySubscriptionVariant, lifecycle::SubscriptionService};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::RuntimeLease, tenant_ref::TenantRef};
use axum::http::Uri;
use futures::future::BoxFuture;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// The complete subscription capability set exposed by one tenant.
#[derive(TypedBuilder)]
pub struct SubscriptionRouteServices {
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
    /// Tenant's ordinary live context branch.
    pub live_target: ContextTarget,
    /// Snapshot URI to durable branch resolution.
    pub snapshot_targets: Arc<dyn SnapshotTargetResolver>,
    /// Entity-subscription lifecycle policy.
    pub subscriptions: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    /// Context-source-subscription lifecycle policy.
    pub csource_subscriptions: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    /// The tenant's JSON-LD processor, which resolves the `@context` a
    /// subscription response names (clause 5.2.3).
    pub engine: JsonLdEngine,
    /// The tenant's host of the contexts its responses and subscriptions name
    /// when no single URL names them (Annex C.8, clause 5.8.1.4).
    pub context_hosting: TenantContextHosting,
}

/// Existing-tenant extraction specialized to subscription capabilities.
pub(crate) type SubscriptionTenantState = ExistingTargetedTenantState<Arc<SubscriptionRouteServices>>;
/// Provisionable-tenant extraction specialized to subscription capabilities.
pub(crate) type SubscriptionTenantStateOrCreate = ProvisionableTargetedTenantState<Arc<SubscriptionRouteServices>>;
/// Existing live tenant state for context-source subscription routes.
pub(crate) type CsourceSubscriptionTenantState = ExistingTenantState<Arc<SubscriptionRouteServices>>;
/// Provisionable live tenant state for context-source subscription routes.
pub(crate) type CsourceSubscriptionTenantStateOrCreate = ProvisionableTenantState<Arc<SubscriptionRouteServices>>;

impl ContextTargetProvider for SubscriptionRouteServices {
    fn live_target(&self) -> &ContextTarget {
        &self.live_target
    }

    fn snapshot_target_resolver(&self) -> &dyn SnapshotTargetResolver {
        self.snapshot_targets.as_ref()
    }
}

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
    /// Resolves or creates the request's tenant with its subscription capabilities.
    resolver: Arc<dyn SubscriptionStateResolver>,
}

impl SubscriptionRouteState {
    /// Route state resolving tenants through `resolver`.
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
                .map_err(|error| ProblemResponse::classified(classify_tenancy_error(&error), instance.clone()))
        })
    }
}
