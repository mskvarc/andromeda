//! Composition of the source identity route and the default tenant's contexts
//! it answers under.

use crate::tenant_factory::BrokerTenantFactory;
use andromeda_configuration::schema::federation::identity::IdentityConfig;
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_source_identity::{
    default_tenant_contexts::{DefaultTenantContexts, DefaultTenantContextsResolver},
    routes::SourceIdentityRoutes,
    state::SourceIdentityState,
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError, configured_extras::ConfiguredExtrasError};
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::{ContextSourcePseudonym, ContextSourcePseudonymError},
    identifier::{Identifier, IdentifierParseError},
};
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
pub enum SourceIdentityRouteError {
    /// The configured `contextSourceExtras` are no admissible JSON.
    #[error("the configured source extras are not admissible JSON: {0}")]
    Extras(#[from] JsonLiteralError),
    /// `identity.source_id` is not an identifier a stored resource can carry.
    #[error("identity.source_id: {0}")]
    Identifier(#[from] IdentifierParseError),
    /// `identity.source_alias` is not a single `received-by` token.
    #[error("identity.source_alias: {0}")]
    Alias(#[from] ContextSourcePseudonymError),
    /// `identity.source_extras` is not a JSON literal.
    #[error(transparent)]
    ConfiguredExtras(#[from] ConfiguredExtrasError),
}

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
/// Returns [`SourceIdentityRouteError::Extras`] for extras the enabled
/// extensions could not be written into, which no admitted
/// `contextSourceExtras` raise, and [`SourceIdentityRouteError::Identifier`]
/// or [`SourceIdentityRouteError::Alias`] for a configured identifier past
/// the bound or an alias that is not a token, both of which loading already refuses.
pub(crate) fn source_identity_routes(
    identity: &IdentityConfig,
    started_at: DateTime<Utc>,
    enabled_extensions: EnabledExtensions,
    tenants: &Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
) -> Result<SourceIdentityRoutes, SourceIdentityRouteError> {
    Ok(SourceIdentityRoutes::new(SourceIdentityState::new(
        Identifier::try_from(&identity.source_id)?,
        // Table 5.2.40-1: the configured alias is the default tenant's, and
        // every other tenant's is derived from it.
        ContextSourceAliases::new(ContextSourcePseudonym::try_from(&identity.source_alias)?),
        started_at,
        identity.extras.as_ref().map(JsonLiteral::try_from).transpose()?,
        enabled_extensions,
        Arc::new(BrokerDefaultTenantContexts::new(Arc::clone(tenants))),
    )?))
}
