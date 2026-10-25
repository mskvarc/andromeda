//! Regular expressions carried by a query, validated once when the query is parsed.
//!
//! NGSI-LD puts a regular expression in the `idPattern` parameter (clause 5.7.2) and in the `~=` / `!~=` operands of a `q` filter (clause 4.9). Both are an untrusted pattern from a request, so both use [`Pattern`].
//!
//! A pattern is evaluated by two engines: the `regex` crate in memory and PostgreSQL's Advanced Regular Expression operators (`~`, `!~`) in the store. Clause 4.9 and Table 5.2.8-1 mandate IEEE 1003.2, which both implement, but each extends it in its own direction. The `regex` crate therefore compiles the pattern as the client wrote it, and the store is handed a [`PortablePattern`] rendered from what the `regex` crate compiled, with every embedded option applied and every class written out. That is what lets `(?i)` fold `Č` to `č` in the store as it does in memory, where PostgreSQL's own case-insensitive option folds only ASCII.
//!
//! [`check_posix_dialect`] decides which patterns are admitted: IEEE 1003.2, less the constructs the `regex` crate reads differently from POSIX.2 (accepting on the `regex` crate's terms alone would let `\b` mean a word boundary where IEEE 1003.2 gives it no meaning), plus the extensions the broker supports.
//!
//! One divergence is settled by configuration rather than refusal, because the construct is POSIX.2 itself: `.` matches a newline in POSIX.2 and in PostgreSQL's default newline-insensitive matching, but not in the `regex` crate's default. The in-memory engine is built with `dot_matches_new_line`, so `name~="a.b"` selects a value spelled `a\nb` on both sides.

use crate::{error::QueryError, portable_pattern::PortablePattern, posix_dialect::check_posix_dialect, subject_start::SubjectStart};
use regex::{Regex, RegexBuilder};
use regex_syntax::{ast::parse::Parser, hir::translate::TranslatorBuilder};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A regular expression from a query parameter or a `q` predicate, compiled at parse time.
///
/// Equality is over the source text, not the compiled automaton: two patterns are the same predicate exactly when the client wrote the same thing. The wire form is the pattern as written, so a resource that stores one (an `EntitySelector.idPattern`, clause 5.2.33, or a Snapshot entity selector, clause 5.2.23) gives back exactly what the client sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Pattern {
    /// The client's text, compiled for the in-memory engine.
    compiled: Regex,
    /// The restatement the store evaluates.
    portable: PortablePattern,
    /// What every match fixes about the text its subject begins with.
    subject_start: SubjectStart,
}

impl Pattern {
    /// Compiles and validates a pattern, rejecting one outside the admitted dialect, and renders the restatement the store evaluates.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::PatternRegex`] when the `regex` crate rejects the pattern, [`QueryError::PatternRepeatedQuantifier`] or [`QueryError::PatternNonPosixConstruct`] when [`check_posix_dialect`] finds a construct outside the admitted dialect, and [`QueryError::PatternTranslation`] when the pattern cannot be translated for rendering.
    pub fn parse(pattern: &str) -> Result<Pattern, QueryError> {
        // `dot_matches_new_line` is PostgreSQL's default and the POSIX.2 reading of `.`; the `regex` crate's default would make one filter select different values in memory than in the store.
        let compiled = RegexBuilder::new(pattern).dot_matches_new_line(true).build()?;
        let ast = Parser::new().parse(pattern).map_err(|_| QueryError::PatternRepeatedQuantifier)?;
        check_posix_dialect(&ast)?;
        // Configured as `compiled` was built, so the restatement renders the reading the in-memory engine runs.
        let hir = TranslatorBuilder::new().dot_matches_new_line(true).build().translate(pattern, &ast)?;
        let portable = PortablePattern::render(&hir)?;
        let subject_start = SubjectStart::of(&hir);
        Ok(Pattern {
            compiled,
            portable,
            subject_start,
        })
    }

    /// The pattern as the client wrote it: the wire form, and what a request forwarded to a Context Source carries.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.compiled.as_str()
    }

    /// The restatement a store evaluates in place of the client's text.
    #[must_use]
    pub const fn portable(&self) -> &PortablePattern {
        &self.portable
    }

    /// Whether every match begins its subject with the same text, which a
    /// store holding the subjects in order reads as one range.
    #[must_use]
    pub const fn subject_start(&self) -> SubjectStart {
        self.subject_start
    }

    /// Whether the pattern matches anywhere in `haystack`.
    #[must_use]
    pub fn is_match(&self, haystack: &str) -> bool {
        self.compiled.is_match(haystack)
    }
}

impl FromStr for Pattern {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<Pattern, QueryError> {
        Pattern::parse(raw)
    }
}

// Parsing lives in `Pattern::parse` so borrowed call sites do not allocate; `#[serde(try_from = "String")]` requires this.
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
        formatter.write_str(self.compiled.as_str())
    }
}

// `#[serde(into = "String")]` demands an `Into<String>`.
impl From<Pattern> for String {
    fn from(pattern: Pattern) -> String {
        pattern.compiled.as_str().to_string()
    }
}

// `regex::Regex` carries no equality of its own, and no derive can produce one
// over the source text of a private field of a foreign type.
impl PartialEq for Pattern {
    fn eq(&self, other: &Pattern) -> bool {
        self.compiled.as_str() == other.compiled.as_str()
    }
}

impl Eq for Pattern {}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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

    /// `(?i)` folds by Unicode simple case folding, so the letters of European
    /// languages outside ASCII compare case-insensitively too.
    #[test]
    fn a_case_insensitive_pattern_folds_letters_outside_ascii() {
        let pattern = Pattern::parse("(?i)^čebela$").expect("valid");
        for subject in ["čebela", "Čebela", "ČEBELA"] {
            assert!(pattern.is_match(subject), "{subject}");
        }
        assert!(!pattern.is_match("cebela"), "folding is not accent stripping");
    }

    /// The wire form stays the client's text; the store evaluates the
    /// restatement instead.
    #[test]
    fn a_case_insensitive_pattern_keeps_its_text_beside_its_restatement() {
        let pattern = Pattern::parse("(?i)čer").expect("valid");
        assert_eq!(pattern.as_str(), "(?i)čer");
        assert_eq!(pattern.portable().as_str(), "[Čč][Ee][Rr]");
        assert_eq!(serde_json::to_string(&pattern).expect("serialises"), "\"(?i)čer\"");
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
