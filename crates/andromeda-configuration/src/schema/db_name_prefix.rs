//! The operator-configured prefix every tenant database name is built from.

use derive_more::{AsRef, Display, Into};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The longest prefix, in bytes.
///
/// A tenant database name is the prefix, an underscore, at least one byte of
/// the tenant's label, an underscore and a 16-character digest suffix, and
/// PostgreSQL keeps 63 bytes of a database name. This is what those 63 bytes
/// leave for the prefix.
pub const DB_NAME_PREFIX_BYTE_LIMIT: usize = 44;

/// A validated PostgreSQL identifier fragment prefixing every tenant database
/// name.
///
/// A tenant database name is this prefix joined to a per-tenant suffix, so the
/// prefix must already be a legal unquoted SQL identifier fragment
/// (`[A-Za-z_][A-Za-z0-9_]*`) or the joined name cannot be used without
/// quoting, and it must leave the suffix room within PostgreSQL's identifier
/// limit ([`DB_NAME_PREFIX_BYTE_LIMIT`]). Validating it here rejects an
/// unusable prefix at startup rather than at the first tenant provision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Into, JsonSchema)]
#[schemars(
    description = "The prefix every tenant database name starts with: an unquoted SQL identifier fragment matching `[A-Za-z_][A-Za-z0-9_]*`, at most 44 bytes long."
)]
#[serde(try_from = "String", into = "String")]
#[as_ref(str)]
pub struct DbNamePrefix(String);

/// Why a value could not be read as a [`DbNamePrefix`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DbNamePrefixError {
    /// The value was empty.
    #[error("database name prefix must not be empty")]
    Empty,
    /// The value is longer than [`DB_NAME_PREFIX_BYTE_LIMIT`].
    #[error("a database name prefix of {length} bytes exceeds the {DB_NAME_PREFIX_BYTE_LIMIT} bytes a prefix may occupy")]
    TooLong {
        /// The length of the refused prefix, in bytes.
        length: usize,
    },
    /// The value held a character outside `[A-Za-z0-9_]`, or began with a digit.
    #[error("database name prefix must match [A-Za-z_][A-Za-z0-9_]*, but holds '{}'", .offending.escape_default())]
    Invalid {
        /// The first character the grammar does not admit at its position.
        offending: char,
    },
}

impl DbNamePrefix {
    /// The prefix as a string slice, for a store that has to spell it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DbNamePrefix {
    /// The built-in tenant database-name prefix.
    ///
    /// `andromeda_tenant` satisfies the `[A-Za-z_][A-Za-z0-9_]*` identifier
    /// grammar this type enforces, so the invariant holds without routing the
    /// literal back through the validating conversion.
    fn default() -> DbNamePrefix {
        DbNamePrefix(String::from("andromeda_tenant"))
    }
}

impl TryFrom<String> for DbNamePrefix {
    type Error = DbNamePrefixError;

    fn try_from(value: String) -> Result<DbNamePrefix, DbNamePrefixError> {
        if value.is_empty() {
            return Err(DbNamePrefixError::Empty);
        }
        let length = value.len();
        if length > DB_NAME_PREFIX_BYTE_LIMIT {
            return Err(DbNamePrefixError::TooLong { length });
        }
        // A leading digit is illegal for an unquoted identifier; every later
        // position also admits digits.
        let offending = value.chars().enumerate().find_map(|(index, character)| match character {
            'A'..='Z' | 'a'..='z' | '_' => None,
            '0'..='9' if index > 0 => None,
            other => Some(other),
        });
        if let Some(offending) = offending {
            return Err(DbNamePrefixError::Invalid { offending });
        }
        Ok(DbNamePrefix(value))
    }
}

impl TryFrom<&str> for DbNamePrefix {
    type Error = DbNamePrefixError;

    fn try_from(value: &str) -> Result<DbNamePrefix, DbNamePrefixError> {
        value.to_string().try_into()
    }
}

impl FromStr for DbNamePrefix {
    type Err = DbNamePrefixError;

    fn from_str(value: &str) -> Result<DbNamePrefix, DbNamePrefixError> {
        value.try_into()
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::db_name_prefix::{DB_NAME_PREFIX_BYTE_LIMIT, DbNamePrefix, DbNamePrefixError};

    #[test]
    fn a_valid_prefix_round_trips() {
        let prefix = DbNamePrefix::try_from("andromeda_tenant".to_string()).expect("valid prefix");
        pretty_assertions::assert_eq!(prefix.as_str(), "andromeda_tenant");
        pretty_assertions::assert_eq!(prefix.to_string(), "andromeda_tenant");
    }

    #[test]
    fn an_empty_prefix_is_rejected() {
        pretty_assertions::assert_eq!(DbNamePrefix::try_from(String::new()), Err(DbNamePrefixError::Empty));
    }

    #[test]
    fn a_prefix_starting_with_a_digit_is_rejected_naming_the_digit() {
        pretty_assertions::assert_eq!(
            DbNamePrefix::try_from("1tenant".to_string()),
            Err(DbNamePrefixError::Invalid { offending: '1' })
        );
    }

    #[test]
    fn a_prefix_with_an_illegal_character_is_rejected_naming_the_character() {
        pretty_assertions::assert_eq!(
            DbNamePrefix::try_from("tenant-db".to_string()),
            Err(DbNamePrefixError::Invalid { offending: '-' })
        );
        let spaced = DbNamePrefix::try_from("tenant db".to_string()).expect_err("a space is not an identifier character");
        pretty_assertions::assert_eq!(spaced.to_string(), "database name prefix must match [A-Za-z_][A-Za-z0-9_]*, but holds ' '");
    }

    #[test]
    fn a_prefix_at_the_limit_is_accepted() {
        let at_limit = "p".repeat(DB_NAME_PREFIX_BYTE_LIMIT);
        pretty_assertions::assert_eq!(DbNamePrefix::try_from(at_limit.as_str()).map(|prefix| prefix.as_str().len()), Ok(44));
    }

    #[test]
    fn a_prefix_one_byte_past_the_limit_is_too_long_and_the_refusal_states_the_limit() {
        let refused = DbNamePrefix::try_from("p".repeat(DB_NAME_PREFIX_BYTE_LIMIT + 1));
        pretty_assertions::assert_eq!(refused, Err(DbNamePrefixError::TooLong { length: 45 }));
        pretty_assertions::assert_eq!(
            refused.err().map(|error| error.to_string()),
            Some("a database name prefix of 45 bytes exceeds the 44 bytes a prefix may occupy".to_owned())
        );
    }

    #[test]
    fn serde_deserialises_through_the_validating_conversion() {
        let ok: DbNamePrefix = serde_json::from_str("\"andromeda_tenant\"").expect("valid prefix");
        pretty_assertions::assert_eq!(ok.as_str(), "andromeda_tenant");
        assert!(serde_json::from_str::<DbNamePrefix>("\"bad-prefix\"").is_err());
    }
}
