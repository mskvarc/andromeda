//! The `received-protocol` of a `Via` element (RFC 9110 clause 7.6.3).

use andromeda_ngsi_ld_lexicon::http_token::is_tchar;
use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// The `received-protocol` a `Via` element reports for one hop (RFC 9110 clause
/// 7.6.3, ETSI GS CIM 009 clause 6.3.18).
///
/// RFC 9110 defines it as `[ protocol-name "/" ] protocol-version`, where the
/// name and version are each an HTTP `token` (`1*tchar`): `1.1`, `HTTP/1.1`.
/// Andromeda never interprets the value. It is read from an incoming `Via` and
/// echoed back verbatim when the chain is extended, so the type validates the
/// grammar without ascribing meaning to name or version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[display("{_0}")]
pub struct ReceivedProtocol(String);

/// Why a string cannot be a [`ReceivedProtocol`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReceivedProtocolError {
    /// The value was empty.
    #[error("received-protocol must not be empty")]
    Empty,
    /// The value holds more than the one `/` between name and version.
    #[error("received-protocol holds more than one '/'")]
    TooManySeparators,
    /// The protocol name or version is empty.
    #[error("received-protocol has an empty protocol name or version")]
    EmptyComponent,
    /// A character outside the HTTP `token` character class.
    #[error("received-protocol holds {} outside the HTTP token characters", offending.escape_unicode())]
    NotAToken {
        /// The first character that is not a `tchar`.
        offending: char,
    },
}

impl ReceivedProtocol {
    /// Validate a value against the `received-protocol` grammar.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedProtocolError::Empty`] for an empty value, and
    /// [`ReceivedProtocolError::TooManySeparators`],
    /// [`ReceivedProtocolError::EmptyComponent`] or
    /// [`ReceivedProtocolError::NotAToken`] when the value is not an optional
    /// `protocol-name` and a `protocol-version` separated by a single `/`, each
    /// an HTTP `token`.
    pub fn new(value: impl Into<String>) -> Result<ReceivedProtocol, ReceivedProtocolError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReceivedProtocolError::Empty);
        }
        // received-protocol = [ protocol-name "/" ] protocol-version, so at most
        // one `/`, and every component is a non-empty run of `tchar`.
        if value.matches('/').count() > 1 {
            return Err(ReceivedProtocolError::TooManySeparators);
        }
        if value.split('/').any(str::is_empty) {
            return Err(ReceivedProtocolError::EmptyComponent);
        }
        if let Some(offending) = value.chars().find(|character| *character != '/' && !is_tchar(*character)) {
            return Err(ReceivedProtocolError::NotAToken { offending });
        }
        Ok(ReceivedProtocol(value))
    }

    /// The received-protocol as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ReceivedProtocol {
    type Error = ReceivedProtocolError;

    fn try_from(value: String) -> Result<ReceivedProtocol, ReceivedProtocolError> {
        ReceivedProtocol::new(value)
    }
}

impl TryFrom<&str> for ReceivedProtocol {
    type Error = ReceivedProtocolError;

    fn try_from(value: &str) -> Result<ReceivedProtocol, ReceivedProtocolError> {
        ReceivedProtocol::new(value)
    }
}

impl FromStr for ReceivedProtocol {
    type Err = ReceivedProtocolError;

    fn from_str(value: &str) -> Result<ReceivedProtocol, ReceivedProtocolError> {
        ReceivedProtocol::new(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::received_protocol::{ReceivedProtocol, ReceivedProtocolError};

    #[test]
    fn a_bare_version_and_a_named_protocol_both_round_trip() {
        pretty_assertions::assert_eq!(ReceivedProtocol::new("1.1").expect("valid").as_str(), "1.1");
        let named = ReceivedProtocol::new("HTTP/1.1").expect("valid");
        pretty_assertions::assert_eq!(named.as_str(), "HTTP/1.1");
        pretty_assertions::assert_eq!(named.to_string(), "HTTP/1.1");
    }

    #[test]
    fn an_empty_value_is_rejected() {
        pretty_assertions::assert_eq!(ReceivedProtocol::new(""), Err(ReceivedProtocolError::Empty));
    }

    #[test]
    fn a_value_that_breaks_the_grammar_is_rejected() {
        // Whitespace, an empty component, more than one `/`, and a non-tchar
        // byte each fall outside `[ protocol-name "/" ] protocol-version`.
        pretty_assertions::assert_eq!(ReceivedProtocol::new("1 1"), Err(ReceivedProtocolError::NotAToken { offending: ' ' }));
        pretty_assertions::assert_eq!(ReceivedProtocol::new("HTTP/"), Err(ReceivedProtocolError::EmptyComponent));
        pretty_assertions::assert_eq!(ReceivedProtocol::new("/1.1"), Err(ReceivedProtocolError::EmptyComponent));
        pretty_assertions::assert_eq!(ReceivedProtocol::new("a/b/c"), Err(ReceivedProtocolError::TooManySeparators));
        pretty_assertions::assert_eq!(ReceivedProtocol::new("HTTP:1.1"), Err(ReceivedProtocolError::NotAToken { offending: ':' }));
    }

    #[test]
    fn parsing_and_conversion_agree_with_the_validating_constructor() {
        let parsed: ReceivedProtocol = "1.1".parse().expect("valid");
        pretty_assertions::assert_eq!(parsed, ReceivedProtocol::try_from("1.1").expect("valid"));
        assert!("HTTP/".parse::<ReceivedProtocol>().is_err());
    }
}
