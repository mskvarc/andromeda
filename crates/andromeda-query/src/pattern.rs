//! Regular expressions carried by a query, validated once when the query is
//! parsed.
//!
//! NGSI-LD puts a regular expression in two places: the `idPattern` parameter,
//! which selects entities by identifier (clause 5.7.2), and the `~=` / `!~=`
//! operands of a `q` filter, which select them by attribute value (clause 4.9).
//! Both are the same thing — an untrusted pattern from a request — so both use
//! this type.
//!
//! # Two dialects
//!
//! A pattern is evaluated by two engines. In memory the broker uses the `regex`
//! crate; in the store PostgreSQL applies its POSIX ARE operators (`~`, `!~`).
//! The two grammars overlap but are not identical, so validation here is
//! necessary but not sufficient: it rejects a pattern that neither engine could
//! run, while a pattern that both accept may still mean subtly different things
//! to each. What it removes is the failure mode where an unvalidated pattern
//! reaches evaluation and is treated as matching nothing, which is
//! indistinguishable from a filter that legitimately excluded every entity.

use crate::error::QueryError;
use regex::Regex;

/// A regular expression from a query parameter or a `q` predicate, compiled at
/// parse time.
///
/// Equality is over the source text, not over the compiled automaton: two
/// patterns are the same predicate exactly when the client wrote the same
/// thing.
#[derive(Debug, Clone)]
pub struct Pattern(Regex);

impl Pattern {
    /// Compile and validate a pattern, rejecting one no engine could run.
    pub fn parse(pattern: &str) -> Result<Pattern, QueryError> {
        Ok(Pattern(Regex::new(pattern)?))
    }

    /// The pattern as the client wrote it.
    ///
    /// This is what binds into SQL, where PostgreSQL compiles it again under
    /// POSIX rules.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Whether the pattern matches anywhere in `haystack`.
    pub fn is_match(&self, haystack: &str) -> bool {
        self.0.is_match(haystack)
    }
}

// `regex::Regex` carries no equality of its own, and no derive can produce one
// over the source text of a private field of a foreign type.
impl PartialEq for Pattern {
    fn eq(&self, other: &Pattern) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

// Equality over the source text is reflexive, symmetric and transitive, so
// the `PartialEq` above is a full equivalence.
impl Eq for Pattern {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_keeps_the_text_the_client_wrote() {
        let pattern = Pattern::parse("^A.*").expect("valid");
        assert_eq!(pattern.as_str(), "^A.*");
    }

    #[test]
    fn an_invalid_regex_pattern_is_rejected_when_the_filter_is_parsed() {
        // An unterminated character class compiles in neither engine. Rejecting
        // it here turns it into a 400; accepting it would make the filter match
        // nothing, which reads as an empty result set.
        let error = Pattern::parse("[").expect_err("must reject");
        assert!(matches!(error, QueryError::PatternRegex(_)), "got {error:?}");
    }

    #[test]
    fn two_patterns_are_equal_exactly_when_their_text_is() {
        assert_eq!(Pattern::parse("^A.*").unwrap(), Pattern::parse("^A.*").unwrap());
        assert_ne!(Pattern::parse("^A.*").unwrap(), Pattern::parse("^B.*").unwrap());
    }

    #[test]
    fn a_pattern_matches_anywhere_in_the_subject() {
        let pattern = Pattern::parse("erc").expect("valid");
        assert!(pattern.is_match("Mercedes"));
        assert!(!pattern.is_match("BMW"));
    }
}
