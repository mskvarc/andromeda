//! Composition of the inbound notification receiver (clauses 5.8.1.4 and 5.8.6).
//!
//! A received notification names a subscription identifier this broker minted;
//! the platform route table says which tenant minted it, and that tenant's
//! runtime holds the forwarding service that handles it.

use crate::{tenant_factory::BrokerTenantFactory, tenant_state::AndromedaState};
use andromeda_http_notification_receiver::state::{NotificationReceiverResolver, ReceiverResolutionError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_subscription_forwarding::{mapping::ForwardedSubscriptionRouter, service::SubscriptionForwardingService};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, runtime::TenantRuntimeManager};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves the tenant that minted a subscription identifier and projects its
/// forwarding service.
pub struct BrokerNotificationReceiverResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
    router: Arc<dyn ForwardedSubscriptionRouter>,
}

impl BrokerNotificationReceiverResolver {
    /// Construct a resolver over the tenant runtime manager and the platform route table.
    pub const fn new(
        tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
        router: Arc<dyn ForwardedSubscriptionRouter>,
    ) -> BrokerNotificationReceiverResolver {
        BrokerNotificationReceiverResolver { tenants, router }
    }
}

impl NotificationReceiverResolver for BrokerNotificationReceiverResolver {
    fn resolve<'a>(&'a self, subscription: &'a Uri) -> BoxFuture<'a, Result<Arc<SubscriptionForwardingService>, ReceiverResolutionError>> {
        Box::pin(async move {
            let tenant = self
                .router
                .tenant_of(subscription)
                .await
                .map_err(|error| ReceiverResolutionError::Tenant { detail: error.to_string() })?
                .ok_or_else(|| ReceiverResolutionError::Unknown {
                    subscription: subscription.clone(), // clone: the error owns the identifier
                })?;
            let state: Arc<AndromedaState> = self
                .tenants
                .resolve_state(&tenant, TenantCreationPolicy::MustExist)
                .await
                .map_err(|error| ReceiverResolutionError::Tenant { detail: error.to_string() })?;
            state
                .subscription_forwarding
                .as_ref()
                .map(Arc::clone)
                .ok_or_else(|| ReceiverResolutionError::ForwardingDisabled {
                    subscription: subscription.clone(), // clone: the error owns the identifier
                })
        })
    }
}
