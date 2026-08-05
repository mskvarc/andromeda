use andromeda_ngsi_ld_lexicon::tenant::TenantName;

/// Controls whether unknown tenants are created on first access or rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantCreationPolicy {
    /// Return error for unknown tenants.
    MustExist,
    /// Create tenant database on first access.
    CreateOnDemand,
}

/// Status of a tenant in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum TenantStatus {
    /// Tenant is active and accepting requests.
    Active,
    /// Tenant is suspended and should not serve requests.
    Suspended,
}

/// Reference to a tenant, either the unnamed default or a named tenant.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TenantRef {
    /// The unnamed default tenant (no `NGSILD-Tenant` header).
    UnnamedDefault,
    /// A named tenant identified by a validated [`TenantName`].
    Named(TenantName),
}

impl TenantRef {
    /// Construct a `TenantRef` from an optional tenant header value.
    pub fn from_header(header: Option<TenantName>) -> TenantRef {
        match header {
            Some(value) => TenantRef::Named(value),
            None => TenantRef::UnnamedDefault,
        }
    }

    /// Return the tenant name if this is a named tenant, or `None` for the unnamed default.
    pub const fn tenant_name(&self) -> Option<&TenantName> {
        match self {
            TenantRef::UnnamedDefault => None,
            TenantRef::Named(name) => Some(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TenantRef, TenantStatus};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use std::str::FromStr;

    #[test]
    fn tenant_status_display_active() {
        pretty_assertions::assert_eq!(TenantStatus::Active.to_string(), "active");
    }

    #[test]
    fn tenant_status_from_str_active() {
        let parsed = match TenantStatus::from_str("active") {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected tenant status parse error: {err}"),
        };
        pretty_assertions::assert_eq!(parsed, TenantStatus::Active);
    }

    #[test]
    fn tenant_name_from_tenant_ref_named() {
        let name = match TenantName::try_from("tenant_a".to_string()) {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected tenant parse error: {err}"),
        };
        let tenant = TenantRef::Named(name);
        let tenant_name = match tenant.tenant_name() {
            Some(value) => value,
            None => unreachable!("expected tenant name"),
        };
        pretty_assertions::assert_eq!(tenant_name.as_str(), "tenant_a");
    }

    #[test]
    fn tenant_name_from_tenant_ref_default() {
        let tenant = TenantRef::UnnamedDefault;
        assert!(tenant.tenant_name().is_none());
    }
}
