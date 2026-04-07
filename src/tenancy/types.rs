/// Error returned when constructing a [`TenantName`] from an empty string.
#[derive(Debug, Clone, thiserror::Error)]
#[error("tenant name must not be empty")]
pub struct EmptyTenantName;

/// A validated, non-empty tenant identifier.
///
/// Wraps a `String` that is guaranteed to be non-empty after trimming.
/// Used in `TenantRef::Named` and the `NGSILD-Tenant` header.
#[derive(Clone, Debug, Eq, PartialEq, Hash, derive_more::Display, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct TenantName(String);

impl TenantName {
    /// Return the tenant name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TenantName {
    type Error = EmptyTenantName;

    fn try_from(s: String) -> Result<TenantName, EmptyTenantName> {
        if s.trim().is_empty() {
            return Err(EmptyTenantName);
        }
        Ok(TenantName(s))
    }
}

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
    pub fn tenant_name(&self) -> Option<&TenantName> {
        match self {
            TenantRef::UnnamedDefault => None,
            TenantRef::Named(name) => Some(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{TenantName, TenantRef, TenantStatus};

    #[test]
    fn tenant_name_try_from_valid() {
        let tenant = match TenantName::try_from("my-tenant".to_string()) {
            Ok(value) => value,
            Err(err) => panic!("unexpected tenant parse error: {err}"),
        };
        assert_eq!(tenant.as_str(), "my-tenant");
    }

    #[test]
    fn tenant_name_try_from_empty() {
        assert!(TenantName::try_from(String::new()).is_err());
    }

    #[test]
    fn tenant_name_try_from_whitespace() {
        assert!(TenantName::try_from("  ".to_string()).is_err());
    }

    #[test]
    fn tenant_status_display_active() {
        assert_eq!(TenantStatus::Active.to_string(), "active");
    }

    #[test]
    fn tenant_status_from_str_active() {
        let parsed = match TenantStatus::from_str("active") {
            Ok(value) => value,
            Err(err) => panic!("unexpected tenant status parse error: {err}"),
        };
        assert_eq!(parsed, TenantStatus::Active);
    }

    #[test]
    fn tenant_name_from_tenant_ref_named() {
        let name = match TenantName::try_from("tenant-a".to_string()) {
            Ok(value) => value,
            Err(err) => panic!("unexpected tenant parse error: {err}"),
        };
        let tenant = TenantRef::Named(name);
        let tenant_name = tenant.tenant_name();
        let tenant_name = match tenant_name {
            Some(value) => value,
            None => panic!("expected tenant name"),
        };
        assert_eq!(tenant_name.as_str(), "tenant-a");
    }

    #[test]
    fn tenant_name_from_tenant_ref_default() {
        let tenant = TenantRef::UnnamedDefault;
        assert!(tenant.tenant_name().is_none());
    }
}
