//! Composition of the source identity route and the default tenant's contexts
//! it answers under.

use crate::{configuration::source_identity::SourceIdentity, tenant_factory::BrokerTenantFactory};
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_source_identity::{
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
use thiserror::Error;

/// Why the `/info/sourceIdentity` route could not be built from the configured identity.
#[derive(Debug, Error)]
pub(crate) enum SourceIdentityRouteError {
    /// The configured `contextSourceExtras` are no admissible JSON.
    #[error("the configured source extras are not admissible JSON: {0}")]
    Extras(#[from] JsonLiteralError),
}

/// Resolves the default tenant and projects the contexts `/info/sourceIdentity`
/// answers under (clause 4.14: the default tenant always exists).
pub(crate) struct BrokerDefaultTenantContexts {
    /// The process tenant runtimes; the default tenant's state is leased per
    /// resolution.
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerDefaultTenantContexts {
    /// Construct the resolver over the process tenant runtime manager.
    pub(crate) const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerDefaultTenantContexts {
        BrokerDefaultTenantContexts { tenants }
    }
}

impl DefaultTenantContextsResolver for BrokerDefaultTenantContexts {
    fn resolve(&self) -> BoxFuture<'_, Result<DefaultTenantContexts, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;
            Ok(DefaultTenantContexts {
                engine: state.entity_query.engine().clone(),
                context_hosting: state.context_hosting.clone(),
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
/// Returns [`SourceIdentityRouteError::Extras`] for extras the enabled
/// extensions could not be written into, which no admitted
/// `contextSourceExtras` raise.
pub(crate) fn source_identity_routes(
    source: SourceIdentity,
    started_at: DateTime<Utc>,
    enabled_extensions: EnabledExtensions,
    tenants: &Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
) -> Result<SourceIdentityRoutes, SourceIdentityRouteError> {
    Ok(SourceIdentityRoutes::new(SourceIdentityState::new(
        source.id,
        // Table 5.2.40-1: the alias in effect is the default tenant's, and
        // every other tenant's is derived from it.
        ContextSourceAliases::new(source.alias),
        started_at,
        source.extras,
        enabled_extensions,
        Arc::new(BrokerDefaultTenantContexts::new(Arc::clone(tenants))),
    )?))
}
