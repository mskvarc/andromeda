//! Parser for the `scopeQ` query parameter (clause 4.19).
//!
//! ```text
//! ScopesQ  = OrScopeQ *(orOp OrScopeQ)
//! OrScopeQ = "(" ScopeQ *(andOp ScopeQ) ")" / ScopeQ
//! ```
//!
//! A pattern's text runs up to the next operator or parenthesis, and `ScopePattern`'s `FromStr` reads it.

use crate::{
    query_text::{
        disjunctive_form::{delimits_item, disjunctive_form},
        grammar_fault::GrammarFault,
    },
    scope::{scope_pattern::ScopePattern, scope_query_error::ScopeQueryError},
};
use winnow::{Parser, stream::LocatingSlice, token::take_till};

/// Parses the text of a `scopeQ` into its alternatives, each the patterns of its conjunction.
///
/// # Errors
///
/// Returns [`ScopeQueryError::MissingPattern`] where an operator, a closing parenthesis or the end of the text stands in place of a pattern,
/// [`ScopeQueryError::UngroupedConjunction`] for a `;` outside a group,
/// [`ScopeQueryError::NestedGroup`] for a disjunction or a group inside a group,
/// [`ScopeQueryError::UnclosedGroup`] for a group left open,
/// [`ScopeQueryError::TrailingInput`] for text after a complete expression,
/// and the pattern's own error for a pattern outside the clause 4.18 and 4.19 grammar.
pub(super) fn parse(text: &str) -> Result<Vec<Vec<ScopePattern>>, ScopeQueryError> {
    disjunctive_form(text, scope_pattern)
}

/// Reads one `ScopeQ` pattern: the text up to the next operator or parenthesis, which `ScopePattern`'s `FromStr` trims and validates. An empty text backtracks, so the form refuses it as a missing pattern.
fn scope_pattern(input: &mut LocatingSlice<&str>) -> Result<ScopePattern, GrammarFault<ScopeQueryError>> {
    take_till(1.., delimits_item).try_map(str::parse::<ScopePattern>).parse_next(input)
}

#[cfg(test)]
mod tests {
    use crate::scope::{parser::parse, scope_query_error::ScopeQueryError};

    #[test]
    fn unicode_whitespace_around_a_pattern_is_trimmed_and_inside_one_is_kept() {
        pretty_assertions::assert_eq!(parse("(\u{2003}/A\u{3000};\u{A0}/B)"), parse("(/A;/B)"));
        assert!(parse("(/A;/B)").is_ok());
        assert_eq!(parse("/A\u{2003}/B").map(drop), Err(ScopeQueryError::InvalidLevel));
    }

    #[test]
    fn a_blank_alternative_or_requirement_is_a_missing_pattern() {
        assert_eq!(parse("/A,\u{2003},/B").map(drop), Err(ScopeQueryError::MissingPattern));
        assert_eq!(parse("(/A;\u{3000})").map(drop), Err(ScopeQueryError::MissingPattern));
    }

    #[test]
    fn a_group_or_a_pattern_after_a_complete_alternative_is_trailing_input() {
        assert_eq!(parse("(/A)(/B)").map(drop), Err(ScopeQueryError::TrailingInput));
        assert_eq!(parse("(/A)/B").map(drop), Err(ScopeQueryError::TrailingInput));
    }

    #[test]
    fn a_pattern_after_a_structural_fault_is_never_read() {
        assert_eq!(parse("(/A,/1B)").map(drop), Err(ScopeQueryError::NestedGroup));
        assert_eq!(parse("/A)/1B").map(drop), Err(ScopeQueryError::TrailingInput));
    }

    #[test]
    fn the_first_invalid_pattern_decides() {
        assert_eq!(parse("/1B,/#/C").map(drop), Err(ScopeQueryError::InvalidLevel));
    }
}
