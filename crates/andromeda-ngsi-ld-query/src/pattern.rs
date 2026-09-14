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
//! Clause 4.9 and Table 5.2.8-1 both mandate IEEE 1003.2, which the two engines
//! implement alike, but each extends it in its own direction and the extensions
//! do not agree.
//!
//! Validation is therefore in two parts, and a pattern has to pass both: the
//! `regex` crate compiles it, which is what the in-memory evaluator will run,
//! and [`check_posix_dialect`] refuses the constructs PostgreSQL's engine either
//! cannot run or reads differently. Accepting a pattern on one engine's terms
//! alone let `\p{L}` reach SQL, where PostgreSQL's refusal surfaced as a
//! `500 InternalError`, and let `\b` mean a word boundary in memory and a
//! backspace in the store.
//!
//! One divergence is settled by configuration rather than by refusal, because
//! the construct is POSIX.2 itself and cannot be turned away: `.` matches a
//! newline in POSIX.2 and in PostgreSQL, whose default is newline-insensitive
//! matching, and does not in the `regex` crate's default. The in-memory engine
//! is therefore built with `dot_matches_new_line`, so `name~="a.b"` selects a
//! value spelled `a\nb` on both sides.
//!
//! What survives is IEEE 1003.2, less the handful of its constructs the two
//! engines cannot be made to agree on, plus the extensions they share.
//! `crate::posix_dialect` names each narrowing and
//! `docs/spec-conformance-sweep.md` records why.

use crate::{error::QueryError, posix_dialect::check_posix_dialect};
use regex::{Regex, RegexBuilder};
use regex_syntax::ast::parse::Parser;
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
    /// Compile and validate a pattern, rejecting one either engine could not
    /// run and one the two would read differently.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::PatternRegex`] when the `regex` crate rejects the
    /// pattern, and [`QueryError::PatternRepeatedQuantifier`] or
    /// [`QueryError::PatternNonPosixConstruct`] when
    /// [`check_posix_dialect`] finds a construct PostgreSQL's POSIX engine does
    /// not share.
    pub fn parse(pattern: &str) -> Result<Pattern, QueryError> {
        // `dot_matches_new_line` is PostgreSQL's default and the POSIX.2
        // reading of `.`; the `regex` crate's default excludes a newline, which
        // would make one filter select different values in memory than in the
        // store.
        let compiled = RegexBuilder::new(pattern).dot_matches_new_line(true).build()?;
        let ast = Parser::new().parse(pattern).map_err(|_| QueryError::PatternRepeatedQuantifier)?;
        check_posix_dialect(&ast)?;
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
    use pretty_assertions::{assert_eq, assert_ne};

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

    /// POSIX.2 gives `.` no newline exception, and PostgreSQL's default is
    /// newline-insensitive matching: `SELECT E'a\nb' ~ 'a.b'` is true. The
    /// `regex` crate's default is the opposite, so the in-memory engine has to
    /// be built to POSIX's reading or one filter means two things.
    #[test]
    fn a_dot_matches_a_newline_as_posix_and_the_store_both_read_it() {
        assert!(Pattern::parse("a.b").expect("valid").is_match("a\nb"));
        assert!(Pattern::parse("^a.b$").expect("valid").is_match("a\nb"));
    }

    /// `^` and `$` anchor the whole subject in both engines unless `(?m)` asks
    /// otherwise, which is the other half of PostgreSQL's newline-insensitive
    /// default.
    #[test]
    fn an_anchor_holds_to_the_whole_subject_until_multi_line_is_asked_for() {
        assert!(!Pattern::parse("^b").expect("valid").is_match("a\nb"));
        assert!(Pattern::parse("(?m)^b").expect("valid").is_match("a\nb"));
    }
}
