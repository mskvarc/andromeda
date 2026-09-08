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
//!
//! The specification bounds how far that can matter. Table 5.2.8-1 defines an
//! `idPattern` as a "Regular expression as per IEEE 1003.2", which is POSIX
//! extended regular expressions — a strict subset of both engines, on which they
//! agree. A pattern that means two things is therefore one that reaches outside
//! what the specification permits a client to write, and the broker answers it
//! consistently within one execution path rather than across all of them.

use crate::error::QueryError;
use regex::Regex;
use regex_syntax::ast::{Ast, parse::Parser};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A regular expression from a query parameter or a `q` predicate, compiled at
/// parse time.
///
/// Equality is over the source text, not over the compiled automaton: two
/// patterns are the same predicate exactly when the client wrote the same
/// thing.
///
/// The wire form is the pattern as written, so a resource that stores one — an
/// `EntitySelector.idPattern` (clause 5.2.33), a Snapshot entity selector
/// (5.2.23) — gives back exactly what the client sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Pattern(Regex);

impl Pattern {
    /// Compile and validate a pattern, rejecting one no engine could run.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::PatternRegex`] when the `regex` crate rejects the
    /// pattern, and [`QueryError::PatternRepeatedQuantifier`] when a repetition
    /// operator is applied directly to another repetition, which PostgreSQL's
    /// POSIX engine rejects.
    pub fn parse(pattern: &str) -> Result<Pattern, QueryError> {
        let compiled = Regex::new(pattern)?;
        let ast = Parser::new().parse(pattern).map_err(|_| QueryError::PatternRepeatedQuantifier)?;
        if has_repeated_quantifier(&ast) {
            return Err(QueryError::PatternRepeatedQuantifier);
        }
        Ok(Pattern(compiled))
    }

    /// The pattern as the client wrote it.
    ///
    /// This is what binds into SQL, where PostgreSQL compiles it again under
    /// POSIX rules.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Whether the pattern matches anywhere in `haystack`.
    #[must_use]
    pub fn is_match(&self, haystack: &str) -> bool {
        self.0.is_match(haystack)
    }
}

/// Return whether an expression applies a repetition operator directly to another repetition.
fn has_repeated_quantifier(ast: &Ast) -> bool {
    match ast {
        Ast::Repetition(repetition) => matches!(repetition.ast.as_ref(), Ast::Repetition(_)) || has_repeated_quantifier(&repetition.ast),
        Ast::Group(group) => has_repeated_quantifier(&group.ast),
        Ast::Alternation(alternation) => alternation.asts.iter().any(has_repeated_quantifier),
        Ast::Concat(concatenation) => concatenation.asts.iter().any(has_repeated_quantifier),
        Ast::Empty(_)
        | Ast::Flags(_)
        | Ast::Literal(_)
        | Ast::Dot(_)
        | Ast::Assertion(_)
        | Ast::ClassUnicode(_)
        | Ast::ClassPerl(_)
        | Ast::ClassBracketed(_) => false,
    }
}

impl FromStr for Pattern {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<Pattern, QueryError> {
        Pattern::parse(raw)
    }
}

// Parsing lives in `Pattern::parse` so borrowed call sites do not allocate;
// this adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for Pattern {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<Pattern, QueryError> {
        Pattern::parse(&raw)
    }
}

// `derive_more::Display` cannot reach through a foreign newtype's private
// field, which is where the source text lives.
impl fmt::Display for Pattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

// The wire form is the pattern as written; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>`.
impl From<Pattern> for String {
    fn from(pattern: Pattern) -> String {
        pattern.0.as_str().to_string()
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
    fn a_repeated_quantifier_rejected_by_postgresql_is_rejected_before_storage() {
        let error = Pattern::parse("invalid_entity_id_pattern**").expect_err("must reject");
        assert!(matches!(error, QueryError::PatternRepeatedQuantifier), "got {error:?}");
    }

    #[test]
    fn two_patterns_are_equal_exactly_when_their_text_is() {
        assert_eq!(Pattern::parse("^A.*").unwrap(), Pattern::parse("^A.*").unwrap());
        assert_ne!(Pattern::parse("^A.*").unwrap(), Pattern::parse("^B.*").unwrap());
    }

    #[test]
    fn a_pattern_round_trips_through_the_text_the_client_wrote() {
        let raw = "urn:ngsi-ld:Vehicle:.*";
        let json = serde_json::to_string(&Pattern::parse(raw).expect("valid")).expect("serialises");
        assert_eq!(json, format!("{raw:?}"));
        let decoded: Pattern = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(decoded, Pattern::parse(raw).expect("valid"));
    }

    #[test]
    fn an_invalid_pattern_is_rejected_when_the_resource_is_deserialized() {
        assert!(serde_json::from_str::<Pattern>("\"[unclosed\"").is_err());
    }

    #[test]
    fn a_pattern_matches_anywhere_in_the_subject() {
        let pattern = Pattern::parse("erc").expect("valid");
        assert!(pattern.is_match("Mercedes"));
        assert!(!pattern.is_match("BMW"));
    }
}
