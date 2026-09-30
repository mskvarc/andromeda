//! Why a `scopeQ` expression or one of its patterns is outside clause 4.19.

use andromeda_ngsi_ld_lexicon::scope::level::LEVEL_CHARACTER_LIMIT;
use thiserror::Error;

/// The clause 4.18 or 4.19 rule a `scopeQ` value broke.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ScopePatternFault {
    /// A pattern named no level at all.
    #[error("a scope pattern must name at least one level")]
    Empty,
    /// A level carries more characters than the label-path index can hold.
    #[error("a scope level carries {characters} characters, more than the {LEVEL_CHARACTER_LIMIT} a level may hold")]
    LevelTooLong {
        /// The number of characters the level carries.
        characters: usize,
    },
    /// A level is outside the clause 4.18 level-name production.
    #[error("a scope level must be a letter followed by letters, digits or underscores")]
    InvalidLevel,
    /// The `#` wildcard stands anywhere but last.
    #[error("the # wildcard may only end a scope pattern")]
    MultiLevelNotLast,
    /// A parenthesised group is not closed.
    #[error("a parenthesised group is not closed")]
    UnclosedGroup,
    /// An operator, a `)` or the end of the input stands where a pattern is
    /// required.
    #[error("a pattern is missing before an operator, a closing parenthesis or the end of the input")]
    MissingPattern,
    /// Input continues after a complete expression: a dangling `)` or a
    /// trailing operator.
    #[error("the expression continues after it is complete")]
    TrailingInput,
}

#[cfg(test)]
mod tests {
    use crate::scope::pattern_fault::ScopePatternFault;

    #[test]
    fn an_overlong_level_reports_its_length_against_the_bound() {
        pretty_assertions::assert_eq!(
            ScopePatternFault::LevelTooLong { characters: 1001 }.to_string(),
            "a scope level carries 1001 characters, more than the 1000 a level may hold"
        );
    }
}
