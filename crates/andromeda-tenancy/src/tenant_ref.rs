//! Which tenant a request is scoped to.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use std::fmt;

/// Reference to a tenant, either the unnamed default or a named tenant.
///
/// A request with no `NGSILD-Tenant` header (clause 6.3.14) addresses the
/// unnamed default, which is a tenant like any other and not the absence of
/// one: it has its own database, its own workers and its own caches, and it is
/// the only one the janitor never evicts.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TenantRef {
    /// The unnamed default tenant.
    UnnamedDefault,
    /// A named tenant identified by a validated [`TenantName`].
    Named(TenantName),
}

impl TenantRef {
    /// The tenant an optional header value addresses.
    #[must_use]
    pub fn from_header(header: Option<TenantName>) -> TenantRef {
        match header {
            Some(value) => TenantRef::Named(value),
            None => TenantRef::UnnamedDefault,
        }
    }

    /// The tenant name, or `None` for the unnamed default.
    #[must_use]
    pub const fn tenant_name(&self) -> Option<&TenantName> {
        match self {
            TenantRef::UnnamedDefault => None,
            TenantRef::Named(name) => Some(name),
        }
    }

    /// Whether this is the unnamed default tenant.
    #[must_use]
    pub const fn is_unnamed_default(&self) -> bool {
        matches!(self, TenantRef::UnnamedDefault)
    }
}

impl fmt::Display for TenantRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TenantRef::UnnamedDefault => f.write_str("<unnamed default>"),
            TenantRef::Named(name) => f.write_str(name.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_ref::TenantRef;
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;

    fn named(raw: &str) -> TenantRef {
        TenantRef::Named(TenantName::try_from(raw.to_string()).expect("a valid tenant name"))
    }

    #[test]
    fn a_request_without_the_header_addresses_the_unnamed_default() {
        let tenant = TenantRef::from_header(None);
        pretty_assertions::assert_eq!(tenant, TenantRef::UnnamedDefault);
        assert!(tenant.tenant_name().is_none());
        assert!(tenant.is_unnamed_default());
    }

    #[test]
    fn a_request_carrying_the_header_addresses_the_tenant_it_names() {
        let tenant = named("tenant_a");
        pretty_assertions::assert_eq!(tenant.tenant_name().map(TenantName::as_str), Some("tenant_a"));
        assert!(!tenant.is_unnamed_default());
    }

    #[test]
    fn the_unnamed_default_renders_as_something_a_log_line_can_carry() {
        pretty_assertions::assert_eq!(TenantRef::UnnamedDefault.to_string(), "<unnamed default>");
        pretty_assertions::assert_eq!(named("tenant_a").to_string(), "tenant_a");
    }
}
