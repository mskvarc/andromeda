//! Focused immutable state for context source identity retrieval.

use crate::default_tenant_contexts::DefaultTenantContextsResolver;
use andromeda_extensions::{advertisement::enabled_extension_extras, enabled::EnabledExtensions};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use andromeda_ngsi_ld_model::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType};
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};

/// What the identity binding knows about this context source.
///
/// Table 5.2.40-1 makes `contextSourceUptime` and `contextSourceTimeAt`
/// mandatory, so the state holds the instant the broker started and builds a
/// fresh document per request rather than a single immutable one. The same
/// table makes `contextSourceAlias` tenant-specific under clause 4.14, so the
/// state holds the whole alias family rather than one alias and derives the
/// answer from the tenant the request addressed.
#[derive(Clone)]
pub struct SourceIdentityState {
    /// The configured identity, shared by every request.
    configured: Arc<ConfiguredIdentity>,
    /// The default tenant's contexts, which the response is resolved and
    /// hosted with (clause 4.14).
    contexts: Arc<dyn DefaultTenantContextsResolver>,
}

/// The operator-configured part of the identity.
struct ConfiguredIdentity {
    /// The identifier of this context source.
    ///
    /// Table 5.2.40-1 restricts `id` to a valid URI and describes it as the
    /// "Context Source ID", making the tenant requirement explicit only for
    /// `contextSourceAlias`. Clause 5.2.40 reads the two together. The type
    /// "represents the data uniquely identifying a Context Source, and if the
    /// Context Source supports multi-tenancy (see clause 4.14) uniquely
    /// identifying a Tenant within that Context Source", so the identifier
    /// names the context source and the alias names the tenant within it. This
    /// one is therefore the same for every tenant.
    id: Identifier,
    /// The pseudonyms placed in `Via` chains, one per tenant.
    aliases: ContextSourceAliases,
    /// The instant the broker became available.
    started_at: DateTime<Utc>,
    /// The operator's raw, never-expanded instance information with the
    /// enabled extensions advertised beside it (clause 5.15.1.4).
    extras: Option<JsonLiteral>,
}

impl SourceIdentityState {
    /// Construct the context source identity exposed by the broker, available
    /// since `started_at`.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLiteralError`] of extras the enabled extensions could
    /// not be written into, which no admitted extras raise.
    pub fn new(
        id: Identifier,
        aliases: ContextSourceAliases,
        started_at: DateTime<Utc>,
        extras: Option<JsonLiteral>,
        enabled: EnabledExtensions,
        contexts: Arc<dyn DefaultTenantContextsResolver>,
    ) -> Result<SourceIdentityState, JsonLiteralError> {
        Ok(SourceIdentityState {
            configured: Arc::new(ConfiguredIdentity {
                id,
                aliases,
                started_at,
                extras: enabled_extension_extras(extras, enabled)?,
            }),
            contexts,
        })
    }

    /// The default tenant's contexts, which the response is resolved and
    /// hosted with.
    #[must_use]
    pub fn contexts(&self) -> &dyn DefaultTenantContextsResolver {
        self.contexts.as_ref()
    }

    /// The identity document `tenant` observes at `now`.
    ///
    /// Clause 5.15.1.1: "In the multi-tenancy use case (see clause 4.14), a
    /// client can obtain identify information about a specific Tenant within a
    /// Context Source." The tenant is the one the `NGSILD-Tenant` header named
    /// (clause 6.3.14), or the default tenant when it named none.
    #[must_use]
    pub fn identity_at(&self, tenant: &TenantRef, now: DateTime<Utc>) -> ContextSourceIdentity {
        let elapsed = (now - self.configured.started_at).to_std().unwrap_or(Duration::ZERO);
        ContextSourceIdentity {
            id: self.configured.id.clone(),
            type_: ContextSourceIdentityType,
            context_source_alias: self.configured.aliases.for_tenant(tenant),
            context_source_uptime: Iso8601Duration::from_std(elapsed),
            context_source_time_at: now,
            context_source_extras: self.configured.extras.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        default_tenant_contexts::{DefaultTenantContexts, DefaultTenantContextsResolver},
        state::SourceIdentityState,
    };
    use andromeda_extensions::{advertisement::EXTENSION_KEY, enabled::EnabledExtensions};
    use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
    use andromeda_ngsi_ld_lexicon::{context_source_alias::ContextSourceAlias, identifier::Identifier, tenant::TenantName};
    use andromeda_tenancy::{context_source_alias::ContextSourceAliases, error::TenancyError, tenant_ref::TenantRef};
    use chrono::{Duration, Utc};
    use futures::future::BoxFuture;
    use std::{error::Error, sync::Arc};

    /// A broker whose default tenant is never reached: the identity document
    /// itself needs none.
    struct NoDefaultTenant;

    impl DefaultTenantContextsResolver for NoDefaultTenant {
        fn resolve(&self) -> BoxFuture<'_, Result<DefaultTenantContexts, TenancyError>> {
            Box::pin(async { Err(TenancyError::MultiTenancyDisabled) })
        }
    }

    fn state() -> Result<SourceIdentityState, Box<dyn Error>> {
        state_with(EnabledExtensions::none())
    }

    fn state_with(enabled: EnabledExtensions) -> Result<SourceIdentityState, Box<dyn Error>> {
        Ok(SourceIdentityState::new(
            "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Identifier>()?,
            ContextSourceAliases::new(ContextSourceAlias::new("test")?),
            Utc::now() - Duration::hours(2),
            Some(node_extras()?),
            enabled,
            Arc::new(NoDefaultTenant),
        )?)
    }

    /// The operator's extras every test configures.
    fn node_extras() -> Result<JsonLiteral, Box<dyn Error>> {
        Ok(JsonLiteral::admit(r#"{"node": 1}"#)?)
    }

    /// The named tenant `raw`.
    fn named(raw: &str) -> Result<TenantRef, Box<dyn Error>> {
        Ok(TenantRef::Named(raw.parse::<TenantName>()?))
    }

    #[test]
    fn the_uptime_grows_with_the_observation_instant() -> Result<(), Box<dyn Error>> {
        let state = state()?;
        let now = Utc::now();
        let earlier = state.identity_at(&TenantRef::UnnamedDefault, now);
        let later = state.identity_at(&TenantRef::UnnamedDefault, now + Duration::minutes(5));
        let earlier_seconds = earlier.context_source_uptime.to_std().ok_or("a fixed-length uptime")?;
        let later_seconds = later.context_source_uptime.to_std().ok_or("a fixed-length uptime")?;
        assert!(later_seconds > earlier_seconds, "{later_seconds:?} must exceed {earlier_seconds:?}");
        pretty_assertions::assert_eq!(later.context_source_time_at, now + Duration::minutes(5));
        Ok(())
    }

    #[test]
    fn an_observation_before_the_start_reports_a_zero_uptime_written_as_pt0s() -> Result<(), Box<dyn Error>> {
        let state = state()?;
        let identity = state.identity_at(&TenantRef::UnnamedDefault, Utc::now() - Duration::days(1));
        pretty_assertions::assert_eq!(identity.context_source_uptime.to_string(), "PT0S");
        Ok(())
    }

    #[test]
    fn the_extras_travel_into_every_document() -> Result<(), Box<dyn Error>> {
        let identity = state()?.identity_at(&TenantRef::UnnamedDefault, Utc::now());
        pretty_assertions::assert_eq!(identity.context_source_extras, Some(node_extras()?));
        Ok(())
    }

    /// Clause 5.15.1.4: the enabled extensions are advertised beside the
    /// operator's own extras, under a vendor namespace rather than `ngsi-ld:`.
    #[test]
    fn enabled_extensions_are_advertised_beside_the_operator_extras() -> Result<(), Box<dyn Error>> {
        let identity = state_with(EnabledExtensions::all())?.identity_at(&TenantRef::UnnamedDefault, Utc::now());
        let extras = identity.context_source_extras.ok_or("extras present")?;
        pretty_assertions::assert_eq!(extras.piece().member("node").map(JsonPiece::text), Some("1"));
        assert!(extras.piece().member(EXTENSION_KEY).is_some(), "{extras}");
        assert!(!EXTENSION_KEY.starts_with("ngsi-ld:"));
        Ok(())
    }

    /// A conformant broker advertises exactly the operator's extras and no
    /// extension key.
    #[test]
    fn a_broker_with_no_extension_advertises_only_the_operator_extras() -> Result<(), Box<dyn Error>> {
        let identity = state_with(EnabledExtensions::none())?.identity_at(&TenantRef::UnnamedDefault, Utc::now());
        pretty_assertions::assert_eq!(identity.context_source_extras, Some(node_extras()?));
        Ok(())
    }

    /// Table 5.2.40-1: the alias identifies a specific Tenant within this
    /// Context Source, so two tenants never observe the same one.
    #[test]
    fn each_tenant_observes_its_own_alias() -> Result<(), Box<dyn Error>> {
        let state = state()?;
        let now = Utc::now();
        let default = state.identity_at(&TenantRef::UnnamedDefault, now);
        let alpha = state.identity_at(&named("alpha")?, now);
        let beta = state.identity_at(&named("beta")?, now);
        pretty_assertions::assert_eq!(default.context_source_alias.as_str(), "test");
        pretty_assertions::assert_eq!(alpha.context_source_alias.as_str(), "test~alpha");
        pretty_assertions::assert_eq!(beta.context_source_alias.as_str(), "test~beta");
        Ok(())
    }

    /// Table 5.2.40-1 restricts `id` to the Context Source ID and makes only
    /// the alias tenant-specific, so the identifier does not move with the
    /// tenant.
    #[test]
    fn the_context_source_identifier_is_the_same_for_every_tenant() -> Result<(), Box<dyn Error>> {
        let state = state()?;
        let now = Utc::now();
        pretty_assertions::assert_eq!(
            state.identity_at(&TenantRef::UnnamedDefault, now).id,
            state.identity_at(&named("alpha")?, now).id
        );
        Ok(())
    }
}
