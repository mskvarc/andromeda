//! Typed `NGSILD-Scope` request header for Andromeda scope-query extensions.
//!
//! Carries the same scope-query language as the `scopeQ` query parameter
//! (clause 4.18): `,` for OR, `;` for AND, and `/`-separated scope path terms.
//! Wildcards `+` (single level) and `#` (multi-level, terminal) are supported.

use andromeda_ngsi_ld_lexicon::scope::level::ScopeLevel;
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
///
/// Delegated to the lexicon's [`ScopeLevel`] production -- the same one the
/// `scopeQ` query parameter validates against -- so the header path accepts
/// exactly what the query path does. A hand-rolled `is_ascii_digit` reading made
/// this stricter, rejecting the Unicode digits clause 4.18's grammar admits.
fn validate_label(segment: &str) -> Result<(), ScopeSyntaxError> {
    ScopeLevel::new(segment)
        .map(|_| ())
        .map_err(|_| ScopeSyntaxError::InvalidLabel(segment.to_string()))
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

    #[test]
    fn a_scope_label_with_a_unicode_digit_is_accepted_like_the_scope_query_path() {
        // Clause 4.18: a scope level is a Unicode letter followed by Unicode
        // letters, digits or underscores -- the same production `scopeQ`
        // validates against. A non-ASCII digit (Devanagari five) must satisfy
        // the scope grammar here too, not be rejected by an ASCII-only reading
        // that made the header's validation stricter than the query path's.
        // (The HTTP header transport itself is ASCII-only, so such a label
        // reaches this validator through the shared grammar, not raw on the
        // wire; the point is that the two paths validate the same language.)
        assert!(NgsiLdScope::parse("/A\u{096B}").is_ok(), "a Unicode-digit label must be accepted");
    }
}
