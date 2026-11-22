//! Why a `scopeQ` expression or one of its patterns is outside clause 4.19.

use crate::query_text::disjunctive_form::DisjunctiveFormErrors;
use andromeda_ngsi_ld_lexicon::scope::level::LEVEL_CHARACTER_LIMIT;
use thiserror::Error;

/// The clause 4.18 or 4.19 rule a `scopeQ` value broke.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ScopeQueryError {
    /// A pattern named no level at all.
    #[error("invalid scopeQ pattern: a scope pattern must name at least one level")]
    EmptyPattern,
    /// A level carries more characters than the label-path index can hold.
    #[error("invalid scopeQ pattern: a scope level carries {characters} characters, more than the {LEVEL_CHARACTER_LIMIT} a level may hold")]
    LevelTooLong {
        /// The number of characters the level carries.
        characters: usize,
    },
    /// A level is outside the clause 4.18 level-name production.
    #[error("invalid scopeQ pattern: a scope level must be a letter followed by letters, digits or underscores")]
    InvalidLevel,
    /// The `#` wildcard stands anywhere but last.
    #[error("invalid scopeQ pattern: the # wildcard may only end a scope pattern")]
    MultiLevelNotLast,
    /// A bare `/#` stands beside another alternative or inside a group;
    /// clause 4.19 admits it only as the whole expression, `ScopesQ =/ "/" "#"`.
    #[error("invalid scopeQ pattern: /# may only stand as the whole scope query")]
    AnyScopeNotAlone,
    /// Two requirements are conjoined outside parentheses; clause 4.19 admits a
    /// conjunction only as `(ScopeQ;ScopeQ)`.
    #[error("invalid scopeQ pattern: a conjunction of scopes must be parenthesised")]
    UngroupedConjunction,
    /// A parenthesised group holds a disjunction or another group; clause 4.19
    /// admits only a conjunction of scope patterns inside one.
    #[error("invalid scopeQ pattern: a parenthesised group may only hold scope patterns joined by ;")]
    NestedGroup,
    /// A parenthesised group is not closed.
    #[error("invalid scopeQ pattern: a parenthesised group is not closed")]
    UnclosedGroup,
    /// An operator, a `)` or the end of the input stands where a pattern is
    /// required.
    #[error("invalid scopeQ pattern: a pattern is missing before an operator, a closing parenthesis or the end of the input")]
    MissingPattern,
    /// Input continues after a complete expression: a `)` with no group open,
    /// or a `(` or a pattern after a complete alternative.
    #[error("invalid scopeQ pattern: the expression continues after it is complete")]
    TrailingInput,
}

impl DisjunctiveFormErrors for ScopeQueryError {
    fn missing_item(_found: Option<char>, _position: usize) -> Self {
        ScopeQueryError::MissingPattern
    }

    fn group_in_group(_position: usize) -> Self {
        ScopeQueryError::NestedGroup
    }

    fn nested_group(_position: usize) -> Self {
        ScopeQueryError::NestedGroup
    }

    fn unclosed_group(_position: usize) -> Self {
        ScopeQueryError::UnclosedGroup
    }

    fn ungrouped_conjunction(_position: usize) -> Self {
        ScopeQueryError::UngroupedConjunction
    }

    fn trailing_input(_found: char, _position: usize) -> Self {
        ScopeQueryError::TrailingInput
    }
}

#[cfg(test)]
mod tests {
    use crate::scope::scope_query_error::ScopeQueryError;

    #[test]
    fn an_overlong_level_reports_its_length_against_the_bound() {
        pretty_assertions::assert_eq!(
            ScopeQueryError::LevelTooLong { characters: 1001 }.to_string(),
            "invalid scopeQ pattern: a scope level carries 1001 characters, more than the 1000 a level may hold"
        );
    }

    #[test]
    fn a_scope_refusal_keeps_the_text_its_wrapper_gave_it() {
        pretty_assertions::assert_eq!(
            ScopeQueryError::EmptyPattern.to_string(),
            "invalid scopeQ pattern: a scope pattern must name at least one level"
        );
    }
}
