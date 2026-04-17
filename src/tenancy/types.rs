use derive_more::Display;
use serde::{Deserialize, Serialize};
use static_regular_grammar::RegularGrammar;

/// Maximum tenant identifier length, chosen to fit inside a Postgres
/// schema/database identifier (63 chars, minus an 8-char hash suffix and
/// separator used by tenant database naming in [`crate::tenancy::database`]).
const MAX_TENANT_LEN: usize = 50;

/// Grammar-validated tenant label (FIWARE convention).
///
/// Matches `[A-Za-z_][A-Za-z0-9_-]*` via ABNF. Length is bounded separately
/// by [`TenantName`] since ABNF regular grammars cannot cheaply encode
/// upper bounds.
#[derive(RegularGrammar, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[grammar(
    file = "grammar/.generated/tenant.abnf",
    sized(TenantLabelBuf, derive(Debug, Display, PartialEq, Eq, PartialOrd, Ord, Hash))
)]
struct TenantLabel(str);

/// Error returned when a tenant identifier fails validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InvalidTenantName {
    /// Input is empty or violates the tenant character grammar.
    #[error("tenant name must match [A-Za-z_][A-Za-z0-9_-]*")]
    InvalidSyntax,
    /// Input exceeds [`MAX_TENANT_LEN`] characters.
    #[error("tenant name must be at most {MAX_TENANT_LEN} characters")]
    TooLong,
}

/// A validated tenant identifier.
///
/// Combines the ABNF grammar check ([`TenantLabel`]) with an upper-bound
/// length limit so that the value is safe to use as a Postgres identifier
/// component.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Display, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantName(TenantLabelBuf);

impl TenantName {
    /// Return the tenant name as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for TenantName {
    type Error = InvalidTenantName;

    fn try_from(value: String) -> Result<TenantName, InvalidTenantName> {
        if value.len() > MAX_TENANT_LEN {
            return Err(InvalidTenantName::TooLong);
        }
        let label = TenantLabelBuf::new(value).map_err(|_| InvalidTenantName::InvalidSyntax)?;
        Ok(TenantName(label))
    }
}

impl From<TenantName> for String {
    fn from(value: TenantName) -> String {
        value.0.into_string()
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
    pub const fn tenant_name(&self) -> Option<&TenantName> {
        match self {
            TenantRef::UnnamedDefault => None,
            TenantRef::Named(name) => Some(name),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use std::str::FromStr;

    use super::{MAX_TENANT_LEN, TenantName, TenantRef, TenantStatus};

    #[test]
    fn tenant_name_valid_plain() {
        let tenant = match TenantName::try_from("tenant1".to_string()) {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected tenant parse error: {err}"),
        };
        pretty_assertions::assert_eq!(tenant.as_str(), "tenant1");
    }

    #[test]
    fn tenant_name_valid_leading_underscore() {
        assert!(TenantName::try_from("_internal".to_string()).is_ok());
    }

    #[test]
    fn tenant_name_valid_mixed_case() {
        assert!(TenantName::try_from("A_B".to_string()).is_ok());
    }

    #[test]
    fn tenant_name_rejects_leading_digit() {
        assert!(TenantName::try_from("1tenant".to_string()).is_err());
    }

    #[test]
    fn tenant_name_valid_with_hyphen() {
        assert!(TenantName::try_from("tenant-a".to_string()).is_ok());
    }

    #[test]
    fn tenant_name_rejects_leading_hyphen() {
        assert!(TenantName::try_from("-tenant".to_string()).is_err());
    }

    #[test]
    fn tenant_name_rejects_space() {
        assert!(TenantName::try_from("a b".to_string()).is_err());
    }

    #[test]
    fn tenant_name_rejects_empty() {
        assert!(TenantName::try_from(String::new()).is_err());
    }

    #[test]
    fn tenant_name_rejects_over_length() {
        let over = "a".repeat(MAX_TENANT_LEN + 1);
        assert!(TenantName::try_from(over).is_err());
    }

    #[test]
    fn tenant_name_accepts_max_length() {
        let at_max = "a".repeat(MAX_TENANT_LEN);
        assert!(TenantName::try_from(at_max).is_ok());
    }

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
        let tenant_name = tenant.tenant_name();
        let tenant_name = match tenant_name {
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
