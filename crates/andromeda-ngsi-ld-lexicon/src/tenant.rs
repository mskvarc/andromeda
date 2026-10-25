//! The tenant identifier the broker keys every request by (ETSI GS CIM 009 clause 6.3.14).

use crate::identifier_length::IDENTIFIER_BYTE_LIMIT;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use static_regular_grammar::RegularGrammar;
use std::str::FromStr;

/// The prefix of the identifier a tenant resource is exposed under,
/// `urn:ngsi-ld:Tenant:{tenantName}`: the Annex A.3 `urn:ngsi-ld:` prefix and the
/// `Tenant` kind.
pub const TENANT_ID_PREFIX: &str = concat!("urn:ngsi-ld:", "Tenant", ":");

/// The greatest length, in UTF-8 octets, a tenant name may have.
///
/// Clause 6.3.14 types the tenant as a String and sets no length, so this is
/// Andromeda's own bound. The registry keeps every name under a unique B-tree
/// index, which is the storage reason [`IDENTIFIER_BYTE_LIMIT`] exists for, and
/// a tenant resource is exposed as [`TENANT_ID_PREFIX`] followed by its name, an
/// identifier held to that same limit. The bound is the identifier limit less
/// the prefix, so the identifier of every admissible tenant fits it. The tenant's
/// database name sets no bound: it is derived with a hash suffix that always
/// fits PostgreSQL's identifier limit.
pub const TENANT_NAME_BYTE_LIMIT: usize = IDENTIFIER_BYTE_LIMIT - TENANT_ID_PREFIX.len();

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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTenantName {
    /// Input is empty or violates the tenant character grammar.
    #[error("tenant name must match [A-Za-z_][A-Za-z0-9_-]*")]
    InvalidSyntax,
    /// Input is longer than [`TENANT_NAME_BYTE_LIMIT`].
    #[error("a tenant name of {length} bytes exceeds the {TENANT_NAME_BYTE_LIMIT} bytes a tenant name may occupy")]
    TooLong {
        /// The length of the refused name, in UTF-8 octets.
        length: usize,
    },
}

/// A validated tenant identifier.
///
/// Combines the ABNF grammar check with [`TENANT_NAME_BYTE_LIMIT`], so the
/// name fits the registry's unique index and the identifier the tenant is
/// exposed under fits [`IDENTIFIER_BYTE_LIMIT`].
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
        let length = value.len();
        if length > TENANT_NAME_BYTE_LIMIT {
            return Err(InvalidTenantName::TooLong { length });
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
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        identifier_length::IDENTIFIER_BYTE_LIMIT,
        tenant::{InvalidTenantName, TENANT_ID_PREFIX, TENANT_NAME_BYTE_LIMIT, TenantName},
    };

    #[test]
    fn tenant_name_valid_plain() {
        let tenant = match TenantName::try_from("tenant1".to_string()) {
            Ok(value) => value,
            Err(err) => panic!("unexpected tenant parse error: {err}"),
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

    /// The identifier `urn:ngsi-ld:Tenant:{name}` of the longest tenant name is
    /// exactly as long as an identifier may be.
    #[test]
    fn the_tenant_name_limit_is_the_identifier_limit_less_the_tenant_id_prefix() {
        pretty_assertions::assert_eq!(TENANT_NAME_BYTE_LIMIT, 1005);
        pretty_assertions::assert_eq!(TENANT_ID_PREFIX.len() + TENANT_NAME_BYTE_LIMIT, IDENTIFIER_BYTE_LIMIT);
    }

    #[test]
    fn a_tenant_name_at_the_limit_is_accepted() {
        let at_limit = format!("t{}", "a".repeat(TENANT_NAME_BYTE_LIMIT - 1));
        let tenant = at_limit.parse::<TenantName>().expect("a tenant name at the limit");
        pretty_assertions::assert_eq!(tenant.as_str().len(), TENANT_NAME_BYTE_LIMIT);
    }

    #[test]
    fn a_tenant_name_one_byte_past_the_limit_is_too_long_and_the_refusal_states_the_limit() {
        let past_limit = "a".repeat(TENANT_NAME_BYTE_LIMIT + 1);
        let refused = past_limit.parse::<TenantName>();
        pretty_assertions::assert_eq!(refused, Err(InvalidTenantName::TooLong { length: 1006 }));
        pretty_assertions::assert_eq!(
            refused.err().map(|error| error.to_string()),
            Some("a tenant name of 1006 bytes exceeds the 1005 bytes a tenant name may occupy".to_owned())
        );
    }

    /// The length is checked before the grammar, so an over-long value is
    /// refused for its length whatever it holds.
    #[test]
    fn an_over_long_value_breaking_the_grammar_is_refused_for_its_length() {
        let past_limit = format!("1{}", "a".repeat(TENANT_NAME_BYTE_LIMIT));
        pretty_assertions::assert_eq!(
            past_limit.parse::<TenantName>(),
            Err(InvalidTenantName::TooLong {
                length: TENANT_NAME_BYTE_LIMIT + 1
            })
        );
    }

    #[test]
    fn a_tenant_name_parses_from_a_borrowed_str() {
        assert!("tenant1".parse::<TenantName>().is_ok());
        assert!("1tenant".parse::<TenantName>().is_err());
    }
}
