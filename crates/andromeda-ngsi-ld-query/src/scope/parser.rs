//! Parser for the `scopeQ` query parameter (clause 4.19).
//!
//! ```text
//! ScopesQ  = OrScopeQ *(orOp OrScopeQ)
//! ScopesQ  =/ "/" "#"
//! OrScopeQ = "(" ScopeQ *1("/" "#") *(andOp ScopeQ *1("/" "#")) ")"
//! OrScopeQ =/ ScopeQ *1("/" "#")
//! ```
//!
//! Clause 4.19 adds the `#` wildcard "at the end of a Scope specification",
//! and a Scope inside a group is one, so a group's patterns may end in `/#`
//! as a lone alternative may, though the clause's grammar writes the group
//! without it.
//!
//! A pattern's text runs up to the next operator or parenthesis, and `ScopePattern`'s `FromStr` reads it.

use crate::{
    query_text::{
        disjunctive_form::{delimits_item, disjunctive_form},
        grammar_fault::GrammarFault,
    },
    scope::{
        scope_pattern::{ScopePattern, spells_any_scope},
        scope_query_error::ScopeQueryError,
    },
};
use winnow::{Parser, stream::LocatingSlice, token::take_till};

/// Parses the text of a `scopeQ` into its alternatives, each the patterns of its conjunction.
///
/// The bare `/#` is the whole expression, read as one alternative of the one
/// pattern `#`.
///
/// # Errors
///
/// Returns [`ScopeQueryError::MissingPattern`] where an operator, a closing parenthesis or the end of the text stands in place of a pattern,
/// [`ScopeQueryError::UngroupedConjunction`] for a `;` outside a group,
/// [`ScopeQueryError::NestedGroup`] for a disjunction or a group inside a group,
/// [`ScopeQueryError::UnclosedGroup`] for a group left open,
/// [`ScopeQueryError::TrailingInput`] for text after a complete expression,
/// [`ScopeQueryError::AnyScopeNotAlone`] for a bare `/#` beside another alternative or inside a group,
/// and the pattern's own error for a pattern outside the clause 4.18 and 4.19 grammar.
pub(super) fn parse(text: &str) -> Result<Vec<Vec<ScopePattern>>, ScopeQueryError> {
    if spells_any_scope(text) {
        return text.parse::<ScopePattern>().map(|any_scope| vec![vec![any_scope]]);
    }
    disjunctive_form(text, scope_pattern)
}

/// Reads one `ScopeQ` pattern: the text up to the next operator or parenthesis, which `ScopePattern`'s `FromStr` trims and validates. An empty text backtracks, so the form refuses it as a missing pattern.
fn scope_pattern(input: &mut LocatingSlice<&str>) -> Result<ScopePattern, GrammarFault<ScopeQueryError>> {
    take_till(1.., delimits_item).try_map(combined_pattern).parse_next(input)
}

/// Reads the text of one pattern of an expression other than the bare `/#`,
/// where clause 4.19 admits no bare `/#`.
fn combined_pattern(text: &str) -> Result<ScopePattern, ScopeQueryError> {
    if spells_any_scope(text) {
        return Err(ScopeQueryError::AnyScopeNotAlone);
    }
    text.parse()
}

#[cfg(test)]
mod tests {
    use crate::scope::{parser::parse, scope_pattern::ScopePattern, scope_query_error::ScopeQueryError};

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
        assert_eq!(parse("/#,/1B").map(drop), Err(ScopeQueryError::AnyScopeNotAlone));
    }

    /// Clause 4.19 writes the bare `/#` only as `ScopesQ =/ "/" "#"`, the
    /// whole expression.
    #[test]
    fn the_bare_any_scope_wildcard_stands_only_as_the_whole_expression() {
        pretty_assertions::assert_eq!(parse(" /# "), parse("/#"));
        assert_eq!(parse("/#").map(|alternatives| alternatives.len()), Ok(1));
        for text in ["(/#;/A)", "(/A;/#)", "(/#)", "( /# )", "/#,/A", "/A|/#", "/#,/#", "(/A;/B),/#"] {
            assert_eq!(parse(text).map(drop), Err(ScopeQueryError::AnyScopeNotAlone), "{text}");
        }
    }

    /// Clause 4.19 lets `#` end every Scope specification, and a Scope inside
    /// a group is one.
    #[test]
    fn a_pattern_inside_a_group_may_end_in_the_multi_level_wildcard() {
        let expected = vec![vec![
            "/Madrid/#".parse::<ScopePattern>().expect("valid pattern"),
            "/CompanyA".parse::<ScopePattern>().expect("valid pattern"),
        ]];
        pretty_assertions::assert_eq!(parse("(/Madrid/#;/CompanyA)"), Ok(expected));
        assert!(parse("(/CompanyA;/Madrid/+/#),/CompanyB/#").is_ok());
    }
}
