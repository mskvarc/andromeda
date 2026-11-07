//! The `contextSourceAlias` this broker answers to, one per tenant.

use crate::tenant_ref::TenantRef;
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;

/// The `contextSourceAlias` values (Table 5.2.40-1) this broker publishes, one
/// for every tenant it serves.
///
/// Table 5.2.40-1 makes the alias "a unique id for a Context Source which can
/// be used to identify loops", and adds that "in the multi-tenancy use case
/// (see clause 4.14), this id shall be identifying a specific Tenant within a
/// registered Context Source". Clause 4.3.6.4 says the same thing from the
/// loop-detection side: "in the case of multi-tenancy (see clause 4.14) each
/// Tenant found within each registered Context Source shall be considered
/// separately". A broker that published one alias for every tenant would make
/// a request that has already traversed one of its tenants look like a loop at
/// all the others, and clause 5.12 would then drop the registrations naming
/// them from the match set.
///
/// The unnamed default tenant keeps the configured alias verbatim. That is not
/// a convenience: an operator's configured alias is the value peer brokers
/// already carry in their registrations, and clause 4.14 makes the default "a
/// Tenant which always exists" rather than a new one, so re-spelling its alias
/// would silently invalidate every registration that names this broker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceAliases {
    /// The alias the unnamed default tenant answers to, exactly as configured.
    default_tenant: ContextSourcePseudonym,
}

impl ContextSourceAliases {
    /// The alias family of a context source whose default tenant is configured
    /// to answer to `default_tenant`.
    #[must_use]
    pub const fn new(default_tenant: ContextSourcePseudonym) -> ContextSourceAliases {
        ContextSourceAliases { default_tenant }
    }

    /// The alias identifying `tenant` within this context source.
    #[must_use]
    pub fn for_tenant(&self, tenant: &TenantRef) -> ContextSourcePseudonym {
        match tenant {
            // clone: the configured alias outlives the derived one, which every
            // caller owns for the length of a response or a tenant runtime.
            TenantRef::UnnamedDefault => self.default_tenant.clone(),
            TenantRef::Named(name) => self.default_tenant.scoped_to_tenant(name),
        }
    }

    /// Whether `alias` names this context source, whichever of its tenants it
    /// identifies: a peer that learned the configured alias from the default
    /// tenant's identity writes it whatever tenant its registration targets.
    #[must_use]
    pub fn names_this_source(&self, alias: &ContextSourcePseudonym) -> bool {
        alias.names_source(&self.default_tenant)
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, tenant::TenantName};

    /// The alias family of a broker configured as `broker-a`.
    fn aliases() -> ContextSourceAliases {
        ContextSourceAliases::new(ContextSourcePseudonym::new("broker-a").expect("a valid pseudonym"))
    }

    /// The named tenant `raw`.
    fn named(raw: &str) -> TenantRef {
        TenantRef::Named(raw.parse::<TenantName>().expect("a valid tenant name"))
    }

    /// The default tenant's alias is the operator's configured value, byte for
    /// byte: peer registrations already carry it.
    #[test]
    fn the_default_tenant_keeps_the_configured_alias_verbatim() {
        pretty_assertions::assert_eq!(aliases().for_tenant(&TenantRef::UnnamedDefault).as_str(), "broker-a");
    }

    /// Table 5.2.40-1: a named tenant's alias identifies that tenant within
    /// this context source.
    #[test]
    fn a_named_tenant_gets_an_alias_identifying_it_within_the_context_source() {
        pretty_assertions::assert_eq!(aliases().for_tenant(&named("alpha")).as_str(), "broker-a~alpha");
    }

    /// Clause 4.3.6.4: each tenant of a registered context source is considered
    /// separately, so no two tenants of this broker may share an alias.
    #[test]
    fn no_two_tenants_of_one_broker_share_an_alias() {
        let aliases = aliases();
        let default = aliases.for_tenant(&TenantRef::UnnamedDefault);
        let alpha = aliases.for_tenant(&named("alpha"));
        let beta = aliases.for_tenant(&named("beta"));
        assert_ne!(default, alpha);
        assert_ne!(default, beta);
        assert_ne!(alpha, beta);
    }

    /// The derivation is a pure function of the configured alias and the
    /// tenant, so two lookups of one tenant always agree: the value goes into
    /// a `Via` chain on one request and is compared against a registration on
    /// the next.
    #[test]
    fn the_alias_of_one_tenant_is_stable_across_lookups() {
        let aliases = aliases();
        pretty_assertions::assert_eq!(aliases.for_tenant(&named("alpha")), aliases.for_tenant(&named("alpha")));
        pretty_assertions::assert_eq!(aliases.for_tenant(&TenantRef::UnnamedDefault), aliases.for_tenant(&TenantRef::UnnamedDefault));
    }

    /// A broker configured with a URI-shaped alias derives URI-shaped tenant
    /// aliases; the derivation never assumes a bare label.
    #[test]
    fn a_uri_shaped_configured_alias_derives_uri_shaped_tenant_aliases() {
        let aliases = ContextSourceAliases::new(ContextSourcePseudonym::new("urn:ngsi-ld:ContextSourceIdentity:broker1").expect("a valid pseudonym"));
        pretty_assertions::assert_eq!(aliases.for_tenant(&named("alpha")).as_str(), "urn:ngsi-ld:ContextSourceIdentity:broker1~alpha");
    }

    #[test]
    fn the_alias_of_every_tenant_names_this_source_and_another_broker_s_does_not() {
        let aliases = aliases();
        assert!(aliases.names_this_source(&aliases.for_tenant(&TenantRef::UnnamedDefault)));
        assert!(aliases.names_this_source(&aliases.for_tenant(&named("alpha"))));
        let other = ContextSourceAliases::new(ContextSourcePseudonym::new("broker-b").expect("a valid pseudonym"));
        assert!(!aliases.names_this_source(&other.for_tenant(&TenantRef::UnnamedDefault)));
        assert!(!aliases.names_this_source(&other.for_tenant(&named("alpha"))));
    }
}
