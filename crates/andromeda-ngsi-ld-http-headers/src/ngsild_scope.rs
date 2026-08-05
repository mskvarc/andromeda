//! Typed `NGSILD-Scope` request header for Andromeda scope-query extensions.
//!
//! Carries the same scope-query language as the `scopeQ` query parameter
//! (clause 4.18): `,` for OR, `;` for AND, and `/`-separated scope path terms.
//! Wildcards `+` (single level) and `#` (multi-level, terminal) are supported.

use http::{HeaderName, HeaderValue};
use thiserror::Error;

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_SCOPE: HeaderName = HeaderName::from_static("ngsild-scope");

/// The `urn:ngsi-ld:null` term, which clause 4.18 admits as a scope in its own
/// right and which therefore bypasses the path grammar entirely.
const NULL_SCOPE: &str = "urn:ngsi-ld:null";

/// Parsed Andromeda scope-query expression carried by `NGSILD-Scope`.
///
/// Holds the expression with its whitespace normalised away, so two headers
/// that mean the same thing compare equal and the value can be handed to the
/// scope-query parser unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdScope(String);

impl NgsiLdScope {
    /// The normalised scope expression.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate a raw expression and normalise its whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeSyntaxError`] describing the first term that does not
    /// satisfy the clause 4.18 grammar.
    fn parse(raw: &str) -> Result<NgsiLdScope, ScopeSyntaxError> {
        let mut disjuncts = Vec::new();
        for disjunct in raw.split(',').map(str::trim) {
            if disjunct.is_empty() {
                return Err(ScopeSyntaxError::EmptyTerm);
            }
            let conjuncts = disjunct
                .split(';')
                .map(str::trim)
                .map(|term| validate_term(term).map(|()| term.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            disjuncts.push(conjuncts.join(";"));
        }
        Ok(NgsiLdScope(disjuncts.join(",")))
    }
}

/// Why a scope expression does not satisfy the clause 4.18 grammar.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScopeSyntaxError {
    /// A comma- or semicolon-delimited term was empty.
    #[error("scope expression contains an empty term")]
    EmptyTerm,
    /// A scope path contained an empty segment, as `//` produces.
    #[error("scope path `{0}` contains an empty segment")]
    EmptySegment(String),
    /// The multi-level wildcard `#` appeared before the end of a path.
    #[error("the `#` wildcard in scope path `{0}` must be its last segment")]
    NonTerminalWildcard(String),
    /// A path segment was not a scope label: a letter followed by letters,
    /// digits or underscores.
    #[error("`{0}` is not a valid scope label")]
    InvalidLabel(String),
}

/// Validate one scope path, which is one conjunct of the expression.
fn validate_term(term: &str) -> Result<(), ScopeSyntaxError> {
    if term.is_empty() {
        return Err(ScopeSyntaxError::EmptyTerm);
    }
    if term == NULL_SCOPE {
        return Ok(());
    }

    let path = term.strip_prefix('/').unwrap_or(term);
    if path.is_empty() {
        return Err(ScopeSyntaxError::EmptyTerm);
    }

    let segments = path.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(ScopeSyntaxError::EmptySegment(term.to_string()));
    }

    for (index, segment) in segments.iter().enumerate() {
        match *segment {
            "+" => continue,
            "#" if index + 1 == segments.len() => continue,
            "#" => return Err(ScopeSyntaxError::NonTerminalWildcard(term.to_string())),
            label => validate_label(label)?,
        }
    }

    Ok(())
}

/// Validate one path segment that is not a wildcard.
fn validate_label(segment: &str) -> Result<(), ScopeSyntaxError> {
    let mut characters = segment.chars();
    let starts_with_letter = characters.next().is_some_and(char::is_alphabetic);
    let rest_is_label = characters.all(|character| character.is_alphabetic() || character.is_ascii_digit() || character == '_');

    if starts_with_letter && rest_is_label {
        Ok(())
    } else {
        Err(ScopeSyntaxError::InvalidLabel(segment.to_string()))
    }
}

impl headers::Header for NgsiLdScope {
    fn name() -> &'static HeaderName {
        &NGSILD_SCOPE
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdScope, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let raw = values
            .next()
            .ok_or_else(headers::Error::invalid)?
            .to_str()
            .map_err(|_| headers::Error::invalid())?
            .trim();
        if raw.is_empty() {
            return Err(headers::Error::invalid());
        }
        NgsiLdScope::parse(raw).map_err(|_| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.as_str()) {
            Ok(value) => values.extend(std::iter::once(value)),
            Err(error) => tracing::warn!(error = %error, "failed to encode NGSILD-Scope header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ngsild_scope::{NgsiLdScope, ScopeSyntaxError};
    use headers::Header;
    use http::HeaderValue;

    fn decode_from(raw: &str) -> Result<NgsiLdScope, headers::Error> {
        let value = HeaderValue::from_str(raw).expect("valid header value");
        let mut values = std::iter::once(&value);
        NgsiLdScope::decode(&mut values)
    }

    #[test]
    fn a_valid_expression_survives_decoding_unchanged() {
        pretty_assertions::assert_eq!(decode_from("/A;/B,/C/#").expect("valid scope").as_str(), "/A;/B,/C/#");
        pretty_assertions::assert_eq!(decode_from("/A/+/C").expect("valid scope").as_str(), "/A/+/C");
    }

    #[test]
    fn whitespace_around_the_operators_is_normalised_away() {
        pretty_assertions::assert_eq!(decode_from(" /A ; /B , /C/# ").expect("valid scope").as_str(), "/A;/B,/C/#");
    }

    #[test]
    fn the_null_scope_bypasses_the_path_grammar() {
        pretty_assertions::assert_eq!(decode_from("urn:ngsi-ld:null").expect("valid scope").as_str(), "urn:ngsi-ld:null");
    }

    #[test]
    fn an_empty_header_is_not_a_scope() {
        assert!(decode_from("").is_err());
        assert!(matches!(NgsiLdScope::parse("/A,,/B"), Err(ScopeSyntaxError::EmptyTerm)));
    }

    #[test]
    fn each_grammar_violation_reports_which_rule_it_broke() {
        assert!(matches!(NgsiLdScope::parse("/A//B"), Err(ScopeSyntaxError::EmptySegment(term)) if term == "/A//B"));
        assert!(matches!(NgsiLdScope::parse("/A/#/B"), Err(ScopeSyntaxError::NonTerminalWildcard(term)) if term == "/A/#/B"));
        assert!(matches!(NgsiLdScope::parse("/1A/B"), Err(ScopeSyntaxError::InvalidLabel(label)) if label == "1A"));
    }

    #[test]
    fn a_grammar_violation_is_rejected_at_the_header() {
        assert!(decode_from("/A//B").is_err());
        assert!(decode_from("/A/#/B").is_err());
        assert!(decode_from("/1A/B").is_err());
    }
}
