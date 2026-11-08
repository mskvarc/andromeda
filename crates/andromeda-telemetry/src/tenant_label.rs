//! The value the `tenant` label carries on the series of one tenant.
//!
//! A named tenant's series carry its name. The unnamed default tenant has no
//! name, so its series carry `@default`: a tenant name starts with a letter or
//! an underscore, so no named tenant can share it, and it needs no escaping in
//! a `PromQL` selector.

use metrics::SharedString;

/// The `tenant` label value of the unnamed default tenant.
pub const UNNAMED_DEFAULT_TENANT: &str = "@default";

/// The `tenant` label value of one tenant's series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantLabel(SharedString);

impl TenantLabel {
    /// The label of the unnamed default tenant, [`UNNAMED_DEFAULT_TENANT`].
    #[must_use]
    pub const fn unnamed_default() -> TenantLabel {
        TenantLabel(SharedString::const_str(UNNAMED_DEFAULT_TENANT))
    }

    /// The label of the tenant called `name`.
    ///
    /// The name is a validated tenant name rendered to text, so it never
    /// equals [`UNNAMED_DEFAULT_TENANT`].
    pub fn named(name: impl Into<String>) -> TenantLabel {
        TenantLabel(SharedString::from(name.into()))
    }

    /// The label value, owned as the metrics facade stores it.
    pub(crate) fn value(&self) -> SharedString {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_label::{TenantLabel, UNNAMED_DEFAULT_TENANT};

    #[test]
    fn the_unnamed_default_carries_a_value_no_tenant_name_can_spell() {
        pretty_assertions::assert_eq!(&*TenantLabel::unnamed_default().value(), UNNAMED_DEFAULT_TENANT);
        pretty_assertions::assert_eq!(UNNAMED_DEFAULT_TENANT, "@default");
    }

    #[test]
    fn a_named_tenant_carries_its_name() {
        pretty_assertions::assert_eq!(&*TenantLabel::named("acme").value(), "acme");
        assert_ne!(TenantLabel::named("default"), TenantLabel::unnamed_default());
    }
}
