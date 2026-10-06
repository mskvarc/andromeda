//! Composition of the source identity route and the default tenant's contexts
//! it answers under.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_source_identity::{
    config::InfoConfig,
    default_tenant_contexts::{DefaultTenantContexts, DefaultTenantContextsResolver},
    routes::SourceIdentityRoutes,
    state::SourceIdentityState,
};
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{
    context_source_alias::ContextSourceAliases,
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::{TenantRuntimeManager, lease::LeasedState},
    tenant_ref::TenantRef,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves the default tenant and projects the contexts `/info/sourceIdentity`
/// answers under (clause 4.14: the default tenant always exists).
pub struct BrokerDefaultTenantContexts {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerDefaultTenantContexts {
    /// Construct the resolver over the process tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerDefaultTenantContexts {
        BrokerDefaultTenantContexts { tenants }
    }
}

impl DefaultTenantContextsResolver for BrokerDefaultTenantContexts {
    fn resolve(&self) -> BoxFuture<'_, Result<DefaultTenantContexts, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;
            Ok(DefaultTenantContexts {
                engine: state.entity_query.engine().clone(),    // clone: the engine shares its loader and caches behind `Arc`s
                context_hosting: state.context_hosting.clone(), // clone: the response owns a handle on the default tenant's hosting
                runtime_lease: lease,
            })
        })
    }
}

/// The `/info/sourceIdentity` route over the configured identity, answering
/// under the default tenant's contexts (clause 4.14).
///
/// # Errors
///
/// Returns the [`JsonLiteralError`] of extras the enabled extensions could not
/// be written into, which no admitted `contextSourceExtras` raise.
pub(crate) fn source_identity_routes(
    identity: &InfoConfig,
    started_at: DateTime<Utc>,
    enabled_extensions: EnabledExtensions,
    tenants: &Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
) -> Result<SourceIdentityRoutes, JsonLiteralError> {
    Ok(SourceIdentityRoutes::new(SourceIdentityState::new(
        identity.source_id.clone(), // clone: configuration and immutable route state own the identifier
        // Table 5.2.40-1: the configured alias is the default tenant's, and
        // every other tenant's is derived from it.
        ContextSourceAliases::new(identity.source_alias.clone()), // clone: configuration and the alias family own the configured alias
        started_at,
        identity.source_extras.clone(), // clone: configuration and immutable route state own the extras
        enabled_extensions,
        Arc::new(BrokerDefaultTenantContexts::new(Arc::clone(tenants))),
    )?))
}
