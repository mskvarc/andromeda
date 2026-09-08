//! The tenant identifier the broker keys every request by (ETSI GS CIM 009 clause 6.3.14).

use derive_more::Display;
use serde::{Deserialize, Serialize};
use static_regular_grammar::RegularGrammar;
use std::str::FromStr;

/// Maximum tenant identifier length. NGSI-LD v1.9.1 clause 6.3.14 does not
/// constrain the length of the `NGSILD-Tenant` header value; this bound is
/// chosen for HTTP-header practicality. The broker's tenant-database naming
/// truncates as needed to fit Postgres's 63-char identifier limit while
/// preserving uniqueness via a hash suffix.
const MAX_TENANT_LEN: usize = 255;

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
    /// Input exceeds the maximum tenant identifier length.
    #[error("tenant name must be at most {MAX_TENANT_LEN} characters")]
    TooLong,
}

/// A validated tenant identifier.
///
/// Combines the ABNF grammar check with an upper-bound
/// length limit so that the value is safe to use as a Postgres identifier
/// component.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Display, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantName(TenantLabelBuf);

impl TenantName {
    /// Return the tenant name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Validating parser: the ABNF grammar check and the length bound are what make
// the newtype worth having, and no derive expresses a fallible parse.
impl FromStr for TenantName {
    type Err = InvalidTenantName;

    fn from_str(value: &str) -> Result<TenantName, InvalidTenantName> {
        if value.len() > MAX_TENANT_LEN {
            return Err(InvalidTenantName::TooLong);
        }
        let label = TenantLabelBuf::new(value.to_string()).map_err(|_| InvalidTenantName::InvalidSyntax)?;
        Ok(TenantName(label))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate twice; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for TenantName {
    type Error = InvalidTenantName;

    fn try_from(value: String) -> Result<TenantName, InvalidTenantName> {
        value.parse()
    }
}

// `derive_more::Into` would yield `From<TenantName> for TenantLabelBuf`, one
// step short of the `String` that `#[serde(into = "String")]` requires.
impl From<TenantName> for String {
    fn from(value: TenantName) -> String {
        value.0.into_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant::{MAX_TENANT_LEN, TenantName};

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
    fn a_tenant_name_parses_from_a_borrowed_str() {
        assert!("tenant1".parse::<TenantName>().is_ok());
        assert!("1tenant".parse::<TenantName>().is_err());
    }
}
