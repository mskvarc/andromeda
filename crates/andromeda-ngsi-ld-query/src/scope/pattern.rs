//! One scope path pattern of a `scopeQ` expression (clause 4.19).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::scope::level::{ScopeLevel, ScopeLevelBuf};
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
/// Validation is [`ScopeLevel`], the derived clause 4.18 production the scope
/// path itself is built from, rather than a second reading of the character
/// classes here. That is what makes the `ltree` translation total — an
/// unvalidated label reaches PostgreSQL as an `lquery` cast and fails there,
/// which is a 500 for what is a malformed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeLabel(ScopeLevelBuf);

impl ScopeLabel {
    /// Validate one level name.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ScopePatternInvalid`] when `raw` is outside the
    /// clause 4.18 level-name production.
    pub fn parse(raw: &str) -> Result<ScopeLabel, QueryError> {
        ScopeLevel::new(raw)
            .map(|level| ScopeLabel(level.to_owned()))
            .map_err(|_| QueryError::ScopePatternInvalid(raw.to_string()))
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
    /// A leading `/` is optional and carries no meaning: every scope is
    /// absolute. `#` matches one or more remaining levels and therefore only
    /// makes sense last.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ScopePatternInvalid`] when the pattern is empty,
    /// carries a level outside the clause 4.18 production, or places `#`
    /// anywhere but last.
    pub fn parse(raw: &str) -> Result<ScopePattern, QueryError> {
        // Exactly one leading slash is stripped: `//B` has an empty first
        // level, which is not a label.
        let trimmed = raw.trim();
        let trimmed = trimmed.strip_prefix('/').unwrap_or(trimmed);
        if trimmed.is_empty() {
            return Err(QueryError::ScopePatternInvalid(raw.to_string()));
        }

        let mut segments = Vec::new();
        let mut parts = trimmed.split('/').peekable();
        while let Some(part) = parts.next() {
            let segment = match part {
                SINGLE_LEVEL_TOKEN => ScopeSegment::SingleLevel,
                MULTI_LEVEL_TOKEN if parts.peek().is_none() => ScopeSegment::MultiLevel,
                MULTI_LEVEL_TOKEN => return Err(QueryError::ScopePatternInvalid(raw.to_string())),
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
    /// Subscription matching used to carry its own reading in which `#`
    /// consumed exactly one further level, so `/A/#` matched `/A/B` and not
    /// `/A/B/C` while the same subscription's stored query matched both.
    #[must_use]
    pub fn matches(&self, scope_path: &str) -> bool {
        let trimmed = scope_path.trim();
        let trimmed = trimmed.strip_prefix('/').unwrap_or(trimmed);
        let levels: Vec<&str> = trimmed.split('/').collect();

        for (index, segment) in self.0.iter().enumerate() {
            match segment {
                // `#` is last by construction, so it consumes the remainder.
                // A prefix `#` (something precedes it) matches the anchor
                // levels[0..index] — already verified above — *or* any
                // descendant of it, so `/Madrid/Gardens/#` matches
                // `/Madrid/Gardens` itself (clause 4.19 Example 2). A bare `/#`
                // (nothing precedes it) is the separate `ScopesQ =/ "/" "#"`
                // production: any non-empty scope, so it needs a level present.
                ScopeSegment::MultiLevel => return if index == 0 { levels.len() > index } else { levels.len() >= index },
                ScopeSegment::SingleLevel => {
                    if levels.get(index).is_none() {
                        return false;
                    }
                }
                ScopeSegment::Label(label) => {
                    if levels.get(index) != Some(&label.as_str()) {
                        return false;
                    }
                }
            }
        }
        levels.len() == self.0.len()
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

    fn segments(raw: &str) -> Vec<ScopeSegment> {
        ScopePattern::parse(raw).expect("valid pattern").segments().to_vec()
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
        assert!(pattern.matches("/A/B"));
        assert!(!pattern.matches("/A"));
        assert!(!pattern.matches("/A/B/C"));
        assert!(!pattern.matches("/A/C"));
    }

    #[test]
    fn the_single_level_wildcard_consumes_exactly_one_level() {
        let pattern = ScopePattern::parse("/A/+/C").expect("valid pattern");
        assert!(pattern.matches("/A/B/C"));
        assert!(!pattern.matches("/A/C"));
        assert!(!pattern.matches("/A/B/D/C"));
    }

    #[test]
    fn a_prefix_multi_level_wildcard_matches_its_anchor_or_any_descendant() {
        // Clause 4.19 Example 2: `/A/#` matches the anchor `/A` itself as well
        // as every path below it. An earlier reading required at least one
        // further level, so it wrongly rejected `/A`.
        let pattern = ScopePattern::parse("/A/#").expect("valid pattern");
        assert!(pattern.matches("/A"));
        assert!(pattern.matches("/A/B"));
        assert!(pattern.matches("/A/B/C"));
        assert!(pattern.matches("/A/B/C/D"));
        assert!(!pattern.matches("/B/C"));
    }

    #[test]
    fn a_bare_multi_level_wildcard_matches_any_non_empty_scope() {
        // The `ScopesQ =/ "/" "#"` production: `/#` matches any scope with at
        // least one level.
        let pattern = ScopePattern::parse("/#").expect("valid pattern");
        assert!(pattern.matches("/A"));
        assert!(pattern.matches("/A/B/C"));
    }

    #[test]
    fn a_pattern_matches_a_path_written_without_its_leading_slash() {
        let pattern = ScopePattern::parse("/Madrid").expect("valid pattern");
        assert!(pattern.matches("Madrid"));
        assert!(pattern.matches("/Madrid"));
    }

    #[test]
    fn a_multi_level_wildcard_is_only_valid_last() {
        // `#` stands for the remainder of the path, so nothing can follow it.
        let error = ScopePattern::parse("/A/#/C").expect_err("must reject");
        assert!(matches!(error, QueryError::ScopePatternInvalid(_)), "got {error:?}");
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
