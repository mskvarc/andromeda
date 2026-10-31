//! This broker as a Context Source, seen from the one tenant a forwarding
//! service serves (clauses 4.3.6.4, 4.14, 6.3.17 and 6.3.18).
//!
//! A distributed operation is a loop when it would reach a Context Source it
//! already passed through, each Tenant of a Context Source counted separately.
//! This broker is the first of them: a request is known to loop before it is
//! sent when the registration's `contextSourceAlias` names this broker and the
//! request lands in the tenant it came from.

use andromeda_http_headers::via::Via;
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};

/// This broker, in the tenant a forwarding service serves.
#[derive(Debug, Clone)]
pub struct LocalSource {
    /// The aliases this broker answers to, one per tenant.
    aliases: ContextSourceAliases,
    /// The tenant the forwarding service serves.
    tenant: TenantRef,
    /// The alias of that tenant: the pseudonym every forwarded request appends
    /// to its `Via` chain (clause 6.3.18).
    alias: ContextSourcePseudonym,
}

impl LocalSource {
    /// This broker answering to `aliases`, serving `tenant`.
    #[must_use]
    pub fn new(aliases: ContextSourceAliases, tenant: TenantRef) -> LocalSource {
        let alias = aliases.for_tenant(&tenant);
        LocalSource { aliases, tenant, alias }
    }

    /// The pseudonym naming this tenant in a `Via` chain.
    #[must_use]
    pub const fn alias(&self) -> &ContextSourcePseudonym {
        &self.alias
    }

    /// Whether a request carrying `incoming_via` already passed through this
    /// tenant.
    #[must_use]
    pub fn already_passed(&self, incoming_via: Option<&Via>) -> bool {
        incoming_via.is_some_and(|via| via.contains_pseudonym(&self.alias))
    }

    /// Whether contacting `registration` would close a loop: its alias is
    /// already in the incoming `Via` chain (clause 4.3.6.4), or it names this
    /// broker and the request would land in this very tenant, the "registered
    /// to redirect back on to the Context Broker" case of clause 6.3.17.
    ///
    /// The tenant a forwarded request lands in is the registration's `tenant`,
    /// or the default tenant when it names none (clause 4.14), whatever tenant
    /// its alias identifies.
    #[must_use]
    pub fn closed_by(&self, registration: &ContextSourceRegistration, incoming_via: Option<&Via>) -> bool {
        let Some(alias) = &registration.access.context_source_alias else {
            return false;
        };
        let lands_here = registration.tenant.as_ref() == self.tenant.tenant_name();
        (lands_here && self.aliases.names_this_source(alias)) || incoming_via.is_some_and(|via| via.contains_pseudonym(alias))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::local_source::LocalSource;
    use andromeda_http_headers::{received_protocol::ReceivedProtocol, via::Via};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, tenant::TenantName};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
    use serde_json::json;

    fn pseudonym(raw: &str) -> ContextSourcePseudonym {
        ContextSourcePseudonym::new(raw).expect("a valid alias")
    }

    fn named(raw: &str) -> TenantRef {
        TenantRef::Named(raw.parse::<TenantName>().expect("a valid tenant name"))
    }

    /// Broker `broker-a`, serving `tenant`.
    fn broker_a(tenant: TenantRef) -> LocalSource {
        LocalSource::new(ContextSourceAliases::new(pseudonym("broker-a")), tenant)
    }

    /// A registration aliased `alias`, targeting `tenant` when one is given.
    fn registration(alias: &str, tenant: Option<&str>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:loop",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "contextSourceAlias": alias,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let Some(tenant) = tenant {
            document["tenant"] = json!(tenant);
        }
        serde_json::from_value(document).expect("a valid registration")
    }

    fn via(alias: &str) -> Via {
        Via::with_entry(ReceivedProtocol::new("1.1").expect("a valid protocol"), pseudonym(alias))
    }

    #[test]
    fn a_named_tenant_appends_its_own_alias_to_the_via_chain() {
        pretty_assertions::assert_eq!(broker_a(TenantRef::UnnamedDefault).alias().as_str(), "broker-a");
        pretty_assertions::assert_eq!(broker_a(named("alpha")).alias().as_str(), "broker-a~alpha");
    }

    #[test]
    fn a_request_whose_via_names_this_tenant_already_passed_it_and_one_naming_another_tenant_did_not() {
        let alpha = broker_a(named("alpha"));
        assert!(alpha.already_passed(Some(&via("broker-a~alpha"))));
        assert!(!alpha.already_passed(Some(&via("broker-a"))));
        assert!(!alpha.already_passed(Some(&via("broker-b"))));
        assert!(!alpha.already_passed(None));
    }

    /// Clauses 4.14 and 6.3.17: a registration naming this broker loops when
    /// its forwarded request lands in the tenant it came from, whichever of
    /// this broker's aliases it carries.
    #[test]
    fn a_registration_naming_this_broker_and_landing_in_this_tenant_closes_a_loop() {
        assert!(broker_a(TenantRef::UnnamedDefault).closed_by(&registration("broker-a", None), None));
        assert!(broker_a(named("alpha")).closed_by(&registration("broker-a", Some("alpha")), None));
        assert!(broker_a(named("alpha")).closed_by(&registration("broker-a~alpha", Some("alpha")), None));
    }

    /// Clause 4.3.6.4 counts each tenant separately: a request this broker
    /// forwards into another of its tenants has not come back.
    #[test]
    fn a_registration_naming_this_broker_but_landing_in_another_tenant_closes_no_loop() {
        assert!(!broker_a(named("alpha")).closed_by(&registration("broker-a", None), None));
        assert!(!broker_a(named("alpha")).closed_by(&registration("broker-a", Some("beta")), None));
        assert!(!broker_a(TenantRef::UnnamedDefault).closed_by(&registration("broker-a~alpha", Some("alpha")), None));
    }

    #[test]
    fn a_registration_aliased_as_a_via_hop_closes_a_loop_and_any_other_does_not() {
        let local = broker_a(TenantRef::UnnamedDefault);
        assert!(local.closed_by(&registration("broker-b", None), Some(&via("broker-b"))));
        assert!(!local.closed_by(&registration("broker-c", None), Some(&via("broker-b"))));
        assert!(!local.closed_by(&registration("broker-c", None), None));
        assert!(!local.closed_by(&registration("broker-ab", None), None));
    }
}
