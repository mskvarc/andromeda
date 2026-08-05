//! One scope path pattern of a `scopeQ` expression (clause 4.18).

use crate::error::QueryError;
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
/// The grammar is the one `andromeda_domain::scope::Scope` enforces per level:
/// a Unicode letter, then Unicode letters, digits or underscores. Validating it
/// here is what makes the `ltree` translation total — an unvalidated label
/// reaches PostgreSQL as an `lquery` cast and fails there, which is a 500 for
/// what is a malformed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeLabel(Box<str>);

impl ScopeLabel {
    /// Validate one level name.
    pub fn parse(raw: &str) -> Result<ScopeLabel, QueryError> {
        let mut chars = raw.chars();
        let starts_well = chars.next().is_some_and(char::is_alphabetic);
        if !starts_well || !chars.all(|ch| ch.is_alphanumeric() || ch == '_') {
            return Err(QueryError::ScopePatternInvalid(raw.to_string()));
        }
        Ok(ScopeLabel(Box::from(raw)))
    }

    /// The label as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A scope path pattern, such as `/Madrid`, `/A/+/C` or `/A/#` (clause 4.18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePattern(Vec<ScopeSegment>);

impl ScopePattern {
    /// Parse one pattern.
    ///
    /// A leading `/` is optional and carries no meaning: every scope is
    /// absolute. `#` matches one or more remaining levels and therefore only
    /// makes sense last.
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

    /// The PostgreSQL `lquery` this pattern matches with.
    ///
    /// Levels are dot-separated labels; `+` becomes `*{1}` and `#` becomes
    /// `*{1,}`. Total by construction, because every label was validated
    /// against the label grammar `ltree` itself accepts.
    pub fn to_lquery(&self) -> String {
        let mut lquery = String::new();
        for (index, segment) in self.0.iter().enumerate() {
            if index > 0 {
                lquery.push('.');
            }
            match segment {
                ScopeSegment::Label(label) => lquery.push_str(label.as_str()),
                ScopeSegment::SingleLevel => lquery.push_str("*{1}"),
                ScopeSegment::MultiLevel => lquery.push_str("*{1,}"),
            }
        }
        lquery
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

    fn lquery(raw: &str) -> String {
        ScopePattern::parse(raw).expect("valid pattern").to_lquery()
    }

    #[test]
    fn scope_wildcards_become_lquery_quantifiers() {
        assert_eq!(lquery("/A/B/C"), "A.B.C");
        assert_eq!(lquery("/A/#"), "A.*{1,}");
        assert_eq!(lquery("/A/+/C"), "A.*{1}.C");
        assert_eq!(lquery("/#"), "*{1,}");
    }

    #[test]
    fn a_leading_slash_is_optional() {
        assert_eq!(lquery("Madrid"), "Madrid");
        assert_eq!(lquery("/Madrid"), "Madrid");
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
        assert_eq!(lquery("/M\u{00fc}nchen/Stra\u{00df}e"), "M\u{00fc}nchen.Stra\u{00df}e");
    }

    #[test]
    fn a_pattern_renders_back_to_the_form_it_was_written_in() {
        for raw in ["/A/B/C", "/A/#", "/A/+/C"] {
            assert_eq!(ScopePattern::parse(raw).expect("valid").to_string(), raw);
        }
    }
}
