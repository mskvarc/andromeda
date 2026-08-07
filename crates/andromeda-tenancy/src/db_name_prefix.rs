//! The operator-configured prefix every tenant database name is built from.

use derive_more::{AsRef, Display, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// A validated PostgreSQL identifier fragment prefixing every tenant database
/// name.
///
/// A tenant database name is this prefix joined to a per-tenant suffix, so the
/// prefix must already be a legal unquoted SQL identifier fragment —
/// `[A-Za-z_][A-Za-z0-9_]*` — or the joined name cannot be used without
/// quoting. Validating it here rejects an unusable prefix at startup rather
/// than at the first tenant provision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Into)]
#[serde(try_from = "String", into = "String")]
#[as_ref(str)]
pub struct DbNamePrefix(String);

/// Why a value could not be read as a [`DbNamePrefix`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DbNamePrefixError {
    /// The value was empty.
    #[error("database name prefix must not be empty")]
    Empty,
    /// The value held a character outside `[A-Za-z0-9_]`, or began with a digit.
    #[error("database name prefix must match [A-Za-z_][A-Za-z0-9_]*: {0:?}")]
    Invalid(String),
}

impl DbNamePrefix {
    /// The prefix as a string slice, for a store that has to spell it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DbNamePrefix {
    type Error = DbNamePrefixError;

    fn try_from(value: String) -> Result<DbNamePrefix, DbNamePrefixError> {
        if value.is_empty() {
            return Err(DbNamePrefixError::Empty);
        }
        // A leading digit is illegal for an unquoted identifier; every later
        // position also admits digits.
        let legal = value.bytes().enumerate().all(|(index, byte)| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => true,
            b'0'..=b'9' => index > 0,
            _ => false,
        });
        if !legal {
            return Err(DbNamePrefixError::Invalid(value));
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
    use crate::db_name_prefix::{DbNamePrefix, DbNamePrefixError};

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
    fn a_prefix_starting_with_a_digit_is_rejected() {
        assert!(DbNamePrefix::try_from("1tenant".to_string()).is_err());
    }

    #[test]
    fn a_prefix_with_an_illegal_character_is_rejected() {
        assert!(DbNamePrefix::try_from("tenant-db".to_string()).is_err());
        assert!(DbNamePrefix::try_from("tenant db".to_string()).is_err());
    }

    #[test]
    fn serde_deserialises_through_the_validating_conversion() {
        let ok: DbNamePrefix = serde_json::from_str("\"andromeda_tenant\"").expect("valid prefix");
        pretty_assertions::assert_eq!(ok.as_str(), "andromeda_tenant");
        assert!(serde_json::from_str::<DbNamePrefix>("\"bad-prefix\"").is_err());
    }
}
