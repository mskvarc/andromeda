//! One scope path pattern of a `scopeQ` expression (clause 4.19).

use crate::{error::QueryError, scope::pattern_fault::ScopePatternFault};
use andromeda_ngsi_ld_lexicon::scope::{
    level::{ScopeLevelBuf, ScopeLevelError, checked_level},
    path::{ScopePath, split_levels},
};
use std::fmt::{self, Write};

/// The `+` wildcard: exactly one level, whatever it is named.
const SINGLE_LEVEL_TOKEN: &str = "+";
/// The `#` wildcard: one or more levels, valid only as the last segment.
const MULTI_LEVEL_TOKEN: &str = "#";

/// One segment of a scope pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeSegment {
    /// A literal level name.
    Label(ScopeLabel),
    /// `+`: exactly one level of any name.
    SingleLevel,
    /// `#`: one or more levels of any name.
    MultiLevel,
}

/// A validated level name inside a scope path.
///
/// Admission is the lexicon's [`checked_level`], the one a scope path admits
/// its levels through, rather than a second reading of the character classes
/// or the length bound here. That is what makes the `ltree` translation total
/// — an unvalidated label reaches PostgreSQL as an `lquery` cast and fails
/// there, which is a 500 for what is a malformed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeLabel(ScopeLevelBuf);

impl ScopeLabel {
    /// Validate one level name.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ScopePatternInvalid`] when `raw` is outside the
    /// clause 4.18 level-name production, or carries more characters than a
    /// label the pattern is answered through can hold — which would otherwise
    /// reach PostgreSQL as an `lquery` cast and fail there.
    pub fn parse(raw: &str) -> Result<ScopeLabel, QueryError> {
        checked_level(raw).map(|level| ScopeLabel(level.to_owned())).map_err(|error| {
            QueryError::ScopePatternInvalid(match error {
                ScopeLevelError::Malformed => ScopePatternFault::InvalidLevel,
                ScopeLevelError::TooLong(characters) => ScopePatternFault::LevelTooLong { characters },
            })
        })
    }

    /// The label as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A scope path pattern, such as `/Madrid`, `/A/+/C` or `/A/#` (clause 4.19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePattern(Vec<ScopeSegment>);

impl ScopePattern {
    /// Parse one pattern.
    ///
    /// The levels are split as a scope path splits them
    /// ([`split_levels`]), so a pattern written without its leading `/` --
    /// clause 4.19 EXAMPLE 5 queries a bare `CompanyB` -- names the same levels
    /// as the rooted one. `#` matches one or more remaining levels and
    /// therefore only makes sense last.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ScopePatternInvalid`] when the pattern is empty,
    /// carries a level outside the clause 4.18 production, or places `#`
    /// anywhere but last.
    pub fn parse(raw: &str) -> Result<ScopePattern, QueryError> {
        let trimmed = raw.trim();
        if split_levels(trimmed).eq([""]) {
            return Err(QueryError::ScopePatternInvalid(ScopePatternFault::Empty));
        }

        let mut segments = Vec::new();
        let mut parts = split_levels(trimmed).peekable();
        while let Some(part) = parts.next() {
            let segment = match part {
                SINGLE_LEVEL_TOKEN => ScopeSegment::SingleLevel,
                MULTI_LEVEL_TOKEN if parts.peek().is_none() => ScopeSegment::MultiLevel,
                MULTI_LEVEL_TOKEN => return Err(QueryError::ScopePatternInvalid(ScopePatternFault::MultiLevelNotLast)),
                // `&str` is unbounded, so a catch-all is the only way to close
                // this match; anything that is not a wildcard is a level name.
                label => ScopeSegment::Label(ScopeLabel::parse(label)?),
            };
            segments.push(segment);
        }
        Ok(ScopePattern(segments))
    }

    /// The segments of the pattern, in the order they were written.
    ///
    /// Every segment is validated, which is what lets a backend translate the
    /// pattern without re-checking it: an unvalidated label reaching PostgreSQL
    /// as an `lquery` cast fails there, which is a 500 for a malformed request.
    #[must_use]
    pub fn segments(&self) -> &[ScopeSegment] {
        &self.0
    }

    /// Whether a scope path satisfies this pattern.
    ///
    /// The in-memory reading of clause 4.19, and the counterpart of the
    /// `lquery` a backend renders: a label matches its own level, `+` matches
    /// exactly one level of any name, and `#` matches one *or more* remaining
    /// levels. A pattern with no `#` matches only a path of the same length.
    ///
    /// The path is read by its levels ([`ScopePath::levels`]), never by its
    /// spelling, so a rooted pattern matches a Scope written without its
    /// leading `/`: clause 4.19 can only spell rooted patterns, and a bare
    /// clause 4.18 Scope would otherwise be unreachable.
    #[must_use]
    pub fn matches(&self, path: &ScopePath) -> bool {
        let mut levels = path.levels();
        for (index, segment) in self.0.iter().enumerate() {
            match segment {
                // `#` is last by construction, so it consumes the remainder.
                // A prefix `#` (something precedes it) matches the anchor the
                // earlier segments verified *or* any descendant of it, so
                // `/Madrid/Gardens/#` matches `/Madrid/Gardens` itself (clause
                // 4.19 Example 2). A bare `/#` (nothing precedes it) is the
                // separate `ScopesQ =/ "/" "#"` production: any non-empty
                // scope, so it needs a level present.
                ScopeSegment::MultiLevel => return index > 0 || levels.next().is_some(),
                ScopeSegment::SingleLevel => {
                    if levels.next().is_none() {
                        return false;
                    }
                }
                ScopeSegment::Label(label) => {
                    if levels.next() != Some(label.as_str()) {
                        return false;
                    }
                }
            }
        }
        levels.next().is_none()
    }
}

// Hand-written because the rendering walks a variable-length segment list and
// re-inserts the separator: `derive_more::Display` takes a format string over
// fields, which cannot express a loop.
impl fmt::Display for ScopePattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.0 {
            formatter.write_char('/')?;
            match segment {
                ScopeSegment::Label(label) => formatter.write_str(label.as_str())?,
                ScopeSegment::SingleLevel => formatter.write_str(SINGLE_LEVEL_TOKEN)?,
                ScopeSegment::MultiLevel => formatter.write_str(MULTI_LEVEL_TOKEN)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::scope::level::LEVEL_CHARACTER_LIMIT;

    fn segments(raw: &str) -> Vec<ScopeSegment> {
        ScopePattern::parse(raw).expect("valid pattern").segments().to_vec()
    }

    fn path(raw: &str) -> ScopePath {
        raw.parse().expect("a valid scope path")
    }

    /// A label beyond the widest one the label-path index can hold is a
    /// malformed `scopeQ`, refused here rather than sent to PostgreSQL as an
    /// `lquery` cast that fails and reports an internal error.
    #[test]
    fn a_label_beyond_the_character_bound_is_a_malformed_pattern() {
        let longest = "a".repeat(LEVEL_CHARACTER_LIMIT);
        assert!(ScopePattern::parse(&format!("/{longest}")).is_ok(), "the bound itself is admitted");
        assert!(matches!(
            ScopePattern::parse(&format!("/{longest}a")),
            Err(QueryError::ScopePatternInvalid(ScopePatternFault::LevelTooLong { characters: 1001 }))
        ));
        assert!(matches!(
            ScopePattern::parse(&format!("/A/{longest}a/C")),
            Err(QueryError::ScopePatternInvalid(_))
        ));
    }

    #[test]
    fn a_pattern_keeps_its_wildcards_apart_from_its_labels() {
        pretty_assertions::assert_eq!(
            segments("/A/+/#"),
            vec![
                ScopeSegment::Label(ScopeLabel::parse("A").expect("label")),
                ScopeSegment::SingleLevel,
                ScopeSegment::MultiLevel,
            ]
        );
    }

    #[test]
    fn a_leading_slash_is_optional() {
        pretty_assertions::assert_eq!(segments("Madrid"), segments("/Madrid"));
    }

    #[test]
    fn a_literal_pattern_matches_only_the_path_it_names() {
        let pattern = ScopePattern::parse("/A/B").expect("valid pattern");
        assert!(pattern.matches(&path("/A/B")));
        assert!(!pattern.matches(&path("/A")));
        assert!(!pattern.matches(&path("/A/B/C")));
        assert!(!pattern.matches(&path("/A/C")));
    }

    #[test]
    fn the_single_level_wildcard_consumes_exactly_one_level() {
        let pattern = ScopePattern::parse("/A/+/C").expect("valid pattern");
        assert!(pattern.matches(&path("/A/B/C")));
        assert!(!pattern.matches(&path("/A/C")));
        assert!(!pattern.matches(&path("/A/B/D/C")));
    }

    #[test]
    fn a_prefix_multi_level_wildcard_matches_its_anchor_or_any_descendant() {
        // Clause 4.19 Example 2: `/A/#` matches the anchor `/A` itself as well
        // as every path below it.
        let pattern = ScopePattern::parse("/A/#").expect("valid pattern");
        assert!(pattern.matches(&path("/A")));
        assert!(pattern.matches(&path("/A/B")));
        assert!(pattern.matches(&path("/A/B/C")));
        assert!(pattern.matches(&path("/A/B/C/D")));
        assert!(!pattern.matches(&path("/B/C")));
    }

    #[test]
    fn a_bare_multi_level_wildcard_matches_any_non_empty_scope() {
        // The `ScopesQ =/ "/" "#"` production: `/#` matches any scope with at
        // least one level.
        let pattern = ScopePattern::parse("/#").expect("valid pattern");
        assert!(pattern.matches(&path("/A")));
        assert!(pattern.matches(&path("/A/B/C")));
    }

    #[test]
    fn a_pattern_matches_a_path_written_without_its_leading_slash() {
        let pattern = ScopePattern::parse("/Madrid").expect("valid pattern");
        assert!(pattern.matches(&path("Madrid")));
        assert!(pattern.matches(&path("/Madrid")));
    }

    /// Clause 4.19 spells only rooted patterns, so each wildcard reaches a
    /// clause 4.18 Scope written without its leading `/` by its levels.
    #[test]
    fn a_rooted_pattern_matches_a_bare_scope() {
        assert!(ScopePattern::parse("/Madrid").expect("valid pattern").matches(&path("Madrid")));
        assert!(ScopePattern::parse("/Madrid/#").expect("valid pattern").matches(&path("Madrid/Gardens")));
        assert!(ScopePattern::parse("/+/Gardens").expect("valid pattern").matches(&path("Madrid/Gardens")));
        assert!(ScopePattern::parse("/#").expect("valid pattern").matches(&path("Madrid")));
        assert!(!ScopePattern::parse("/Madrid").expect("valid pattern").matches(&path("Madrid/Gardens")));
    }

    #[test]
    fn a_multi_level_wildcard_is_only_valid_last() {
        // `#` stands for the remainder of the path, so nothing can follow it.
        let error = ScopePattern::parse("/A/#/C").expect_err("must reject");
        assert!(
            matches!(error, QueryError::ScopePatternInvalid(ScopePatternFault::MultiLevelNotLast)),
            "got {error:?}"
        );
    }

    #[test]
    fn a_label_outside_the_scope_grammar_is_rejected() {
        // Clause 4.18: a level starts with a letter and continues with letters,
        // digits or underscores.
        for raw in ["/A B", "/1Floor", "/_hidden", "/A-B", "//B", "/A/"] {
            assert!(ScopePattern::parse(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn an_empty_pattern_is_rejected() {
        assert!(ScopePattern::parse("").is_err());
        assert!(ScopePattern::parse("/").is_err());
    }

    #[test]
    fn unicode_level_names_are_accepted() {
        pretty_assertions::assert_eq!(segments("/M\u{00fc}nchen/Stra\u{00df}e").len(), 2);
    }

    #[test]
    fn a_pattern_renders_back_to_the_form_it_was_written_in() {
        for raw in ["/A/B/C", "/A/#", "/A/+/C"] {
            assert_eq!(ScopePattern::parse(raw).expect("valid").to_string(), raw);
        }
    }
}
