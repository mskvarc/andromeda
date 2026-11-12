//! The `received-by` naming one intermediary in a `Via` chain (RFC 9110 clause 7.6.3).

use crate::context_source_alias::ContextSourceAlias;
use derive_more::{Display, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The `received-by` of one hop in a `Via` header chain (RFC 9110 clause
/// 7.6.3, ETSI GS CIM 009 clause 6.3.18), and the `contextSourceAlias` a
/// registration carries.
///
/// Forwarding loop detection compares these with each other and with this
/// broker's own [`ContextSourceAlias`]. The type is deliberately permissive: an
/// incoming `Via` from another broker may name its hop by a pseudonym
/// (`broker-a`) or by a host and port (`example.com:8080`), and RFC 7230 section
/// 5.7.1 admits both. The only structure required is that a `Via` element can
/// carry the value at all. RFC 9110 makes a `received-by` a single
/// whitespace-delimited token, so the value must be non-empty and free of
/// whitespace and control characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Into, Serialize, Deserialize)]
#[display("{_0}")]
#[into(String)]
#[serde(try_from = "String", into = "String")]
pub struct ContextSourcePseudonym(String);

/// Why a string cannot be a [`ContextSourcePseudonym`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextSourcePseudonymError {
    /// The token was empty.
    #[error("via pseudonym must not be empty")]
    Empty,
    /// The token carried whitespace or a control character, so it could not be a
    /// single `received-by` token.
    #[error("via pseudonym contains the whitespace or control character {}", .offending.escape_unicode())]
    NotAToken {
        /// The first character that cannot appear in a `received-by` token.
        offending: char,
    },
}

impl ContextSourcePseudonym {
    /// Validate an opaque token as a `Via` pseudonym.
    ///
    /// # Errors
    ///
    /// Returns [`ContextSourcePseudonymError::Empty`] for an empty value and
    /// [`ContextSourcePseudonymError::NotAToken`] when the value carries
    /// whitespace or a control character.
    pub fn new(token: impl Into<String>) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        let token = token.into();
        if token.is_empty() {
            return Err(ContextSourcePseudonymError::Empty);
        }
        if let Some(offending) = token.chars().find(|character| character.is_whitespace() || character.is_control()) {
            return Err(ContextSourcePseudonymError::NotAToken { offending });
        }
        Ok(ContextSourcePseudonym(token))
    }

    /// The pseudonym as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// No derive copies one validated newtype into another. The conversion is
// total: an alias is made of `tchar` only, none of which is whitespace or a
// control character.
impl From<&ContextSourceAlias> for ContextSourcePseudonym {
    fn from(alias: &ContextSourceAlias) -> ContextSourcePseudonym {
        ContextSourcePseudonym(alias.as_str().to_owned())
    }
}

impl TryFrom<String> for ContextSourcePseudonym {
    type Error = ContextSourcePseudonymError;

    fn try_from(value: String) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        ContextSourcePseudonym::new(value)
    }
}

impl TryFrom<&str> for ContextSourcePseudonym {
    type Error = ContextSourcePseudonymError;

    fn try_from(value: &str) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        ContextSourcePseudonym::new(value)
    }
}

impl FromStr for ContextSourcePseudonym {
    type Err = ContextSourcePseudonymError;

    fn from_str(value: &str) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        ContextSourcePseudonym::new(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_source_alias::ContextSourceAlias,
        context_source_pseudonym::{ContextSourcePseudonym, ContextSourcePseudonymError},
    };

    #[test]
    fn a_bare_label_is_accepted_and_round_trips() {
        let pseudonym = ContextSourcePseudonym::new("broker-a").expect("valid pseudonym");
        pretty_assertions::assert_eq!(pseudonym.as_str(), "broker-a");
        pretty_assertions::assert_eq!(pseudonym.to_string(), "broker-a");
    }

    /// RFC 7230 section 5.7.1: `received-by = ( uri-host [ ":" port ] ) /
    /// pseudonym`, so a hop another broker names by host and port is read.
    #[test]
    fn a_host_and_port_or_a_uri_shaped_token_is_accepted() {
        let urn = ContextSourcePseudonym::new("urn:ngsi-ld:ContextSourceIdentity:broker1").expect("valid pseudonym");
        pretty_assertions::assert_eq!(urn.as_str(), "urn:ngsi-ld:ContextSourceIdentity:broker1");
        let host = ContextSourcePseudonym::new("example.com:8080").expect("valid pseudonym");
        pretty_assertions::assert_eq!(host.as_str(), "example.com:8080");
    }

    #[test]
    fn an_empty_token_is_rejected() {
        pretty_assertions::assert_eq!(ContextSourcePseudonym::new(""), Err(ContextSourcePseudonymError::Empty));
    }

    #[test]
    fn a_token_with_whitespace_or_a_control_character_is_rejected() {
        for (token, offending) in [("broker a", ' '), ("broker\ta", '\t'), ("broker\u{0007}a", '\u{0007}')] {
            pretty_assertions::assert_eq!(
                ContextSourcePseudonym::new(token),
                Err(ContextSourcePseudonymError::NotAToken { offending }),
                "{token:?} must name its first non-token character"
            );
        }
    }

    #[test]
    fn parsing_and_conversion_agree_with_the_validating_constructor() {
        let parsed: ContextSourcePseudonym = "broker-b".parse().expect("valid pseudonym");
        pretty_assertions::assert_eq!(parsed, ContextSourcePseudonym::try_from("broker-b").expect("valid pseudonym"));
        assert!("".parse::<ContextSourcePseudonym>().is_err());
    }

    #[test]
    fn every_alias_is_a_pseudonym_as_written() {
        for raw in ["broker-a", "broker-a~alpha", "a_b~c!#$%&'*+^`|"] {
            let alias = ContextSourceAlias::new(raw).expect("a valid alias");
            pretty_assertions::assert_eq!(
                ContextSourcePseudonym::from(&alias),
                ContextSourcePseudonym::new(raw).expect("a valid pseudonym")
            );
        }
    }
}
