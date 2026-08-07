//! The pseudonym naming one intermediary in a `Via` chain (RFC 9110 clause 7.6.3).

use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// The pseudonym of one hop in a `Via` header chain (RFC 9110 clause 7.6.3,
/// ETSI GS CIM 009 clause 6.3.18).
///
/// This is the `received-by` token a broker writes to name itself in the `Via`
/// chain and the value forwarding loop detection compares against
/// ([`Via::contains_pseudonym`](crate::via::Via::contains_pseudonym)). It is
/// deliberately permissive: an incoming `Via` from another broker may carry any
/// opaque token -- a bare label (`broker-a`), a host (`example.com:8080`), or a
/// URI (`urn:ngsi-ld:ContextSourceIdentity:broker1`) -- so the only structure
/// required is that a `Via` element can carry the value at all. RFC 9110 makes a
/// `received-by` a single whitespace-delimited token, so the value must be
/// non-empty and free of whitespace and control characters; being URI-shaped is
/// allowed but never required.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[display("{_0}")]
pub struct ContextSourcePseudonym(String);

/// Why a string cannot be a [`ContextSourcePseudonym`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextSourcePseudonymError {
    /// The token was empty.
    #[error("Via pseudonym must not be empty")]
    Empty,
    /// The token carried whitespace or a control character, so it could not be a
    /// single `received-by` token.
    #[error("Via pseudonym `{0}` contains whitespace or a control character")]
    NotAToken(String),
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
        if token.chars().any(|character| character.is_whitespace() || character.is_control()) {
            return Err(ContextSourcePseudonymError::NotAToken(token));
        }
        Ok(ContextSourcePseudonym(token))
    }

    /// The pseudonym as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
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
    use crate::context_source_pseudonym::{ContextSourcePseudonym, ContextSourcePseudonymError};

    #[test]
    fn a_bare_label_is_accepted_and_round_trips() {
        let pseudonym = ContextSourcePseudonym::new("broker-a").expect("valid pseudonym");
        pretty_assertions::assert_eq!(pseudonym.as_str(), "broker-a");
        pretty_assertions::assert_eq!(pseudonym.to_string(), "broker-a");
    }

    #[test]
    fn a_uri_shaped_token_is_accepted_but_not_required() {
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
        assert!(matches!(
            ContextSourcePseudonym::new("broker a"),
            Err(ContextSourcePseudonymError::NotAToken(_))
        ));
        assert!(matches!(
            ContextSourcePseudonym::new("broker\ta"),
            Err(ContextSourcePseudonymError::NotAToken(_))
        ));
        assert!(matches!(
            ContextSourcePseudonym::new("broker\u{0007}a"),
            Err(ContextSourcePseudonymError::NotAToken(_))
        ));
    }

    #[test]
    fn parsing_and_conversion_agree_with_the_validating_constructor() {
        let parsed: ContextSourcePseudonym = "broker-b".parse().expect("valid pseudonym");
        pretty_assertions::assert_eq!(parsed, ContextSourcePseudonym::try_from("broker-b").expect("valid pseudonym"));
        assert!("".parse::<ContextSourcePseudonym>().is_err());
    }
}
