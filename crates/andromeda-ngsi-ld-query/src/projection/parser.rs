//! Parser for the `pick` and `omit` projection list (clause 4.21).
//!
//! ```text
//! ProjectionTerm = AttrName *1(LinkedEntityTerm) *(orOp ProjectionTerm)
//! LinkedEntityTerm = %x7B ProjectionTerm %x7D
//! orOp = %x7C / %x2C
//! ```
//!
//! Names are trimmed of ASCII whitespace, nesting has no depth bound, and every position is a byte offset into the whole text.

use crate::{
    projection::{projection_error::ProjectionError, written_projection_term::WrittenProjectionTerm},
    query_text::{
        grammar_fault::GrammarFault,
        or_operator::{OR_OPERATORS, or_operator},
        whitespace::Whitespace,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use winnow::{
    Parser,
    combinator::{opt, separated},
    stream::{LocatingSlice, Location, Stream},
    token::{one_of, take_till},
};

/// Parses the text of a projection list into its terms.
///
/// # Errors
///
/// Returns [`ProjectionError::EmptyTerm`] where a name is required and none is written,
/// [`ProjectionError::InvalidAttributeName`] for a name outside the attribute-name grammar,
/// and [`ProjectionError::UnbalancedBrace`] when a brace opens without a matching close, closes without a matching open, or is followed by content outside the grammar.
pub(super) fn parse(text: &str) -> Result<Vec<WrittenProjectionTerm>, ProjectionError> {
    let mut input = LocatingSlice::new(text);
    list_to_end(&mut input).map_err(|fault| fault.finish(|position| ProjectionError::EmptyTerm { position }))
}

/// Reads the terms and then requires the end of the text.
fn list_to_end(input: &mut LocatingSlice<&str>) -> Result<Vec<WrittenProjectionTerm>, GrammarFault<ProjectionError>> {
    let terms = term_list(input)?;
    let position = input.current_token_start();
    match input.peek_token() {
        None => Ok(terms),
        Some(_) => Err(GrammarFault::Refused(ProjectionError::UnbalancedBrace { position })),
    }
}

/// Reads `ProjectionTerm *(orOp ProjectionTerm)`, stopping at the first character after a term that is not an `orOp`.
fn term_list(input: &mut LocatingSlice<&str>) -> Result<Vec<WrittenProjectionTerm>, GrammarFault<ProjectionError>> {
    separated(1.., term, or_operator::<ProjectionError>).parse_next(input)
}

/// Reads one `AttrName *1(LinkedEntityTerm)`. Every refusal commits, so a separator never gives its term back.
fn term(input: &mut LocatingSlice<&str>) -> Result<WrittenProjectionTerm, GrammarFault<ProjectionError>> {
    let (text, span) = take_till::<_, _, GrammarFault<ProjectionError>>(0.., delimits_name)
        .with_span()
        .parse_next(input)?;
    let written = Whitespace::Ascii.trim(text);
    if written.is_empty() {
        return Err(GrammarFault::Refused(ProjectionError::EmptyTerm { position: span.start }));
    }
    let name = written
        .parse::<AttributeName>()
        .map_err(|source| GrammarFault::Refused(ProjectionError::InvalidAttributeName(source)))?;
    let linked = opt(linked_entity_term).parse_next(input)?.unwrap_or_default();
    Ok(WrittenProjectionTerm { name, linked })
}

/// Reads `%x7B ProjectionTerm %x7D`. It backtracks when the next character is not `{`. An unclosed brace is reported at the `{` that opened it, the character missing its counterpart.
fn linked_entity_term(input: &mut LocatingSlice<&str>) -> Result<Vec<WrittenProjectionTerm>, GrammarFault<ProjectionError>> {
    let brace = input.current_token_start();
    one_of::<_, _, GrammarFault<ProjectionError>>('{').parse_next(input)?;
    if input.peek_token().is_none() {
        return Err(GrammarFault::Refused(ProjectionError::UnbalancedBrace { position: brace }));
    }
    let children = term_list(input)?;
    match input.peek_token() {
        Some('}') => {
            input.next_token();
            Ok(children)
        }
        Some(_) | None => Err(GrammarFault::Refused(ProjectionError::UnbalancedBrace { position: brace })),
    }
}

/// Whether `character` ends a name: an `orOp` or a brace.
fn delimits_name(character: char) -> bool {
    OR_OPERATORS.contains(&character) || matches!(character, '{' | '}')
}

#[cfg(test)]
mod tests {
    use crate::projection::{parser::parse, projection_error::ProjectionError};

    #[test]
    fn a_position_counts_bytes() {
        let unclosed = parse("ä{").expect_err("must reject");
        assert!(matches!(unclosed, ProjectionError::UnbalancedBrace { position: 2 }), "got {unclosed:?}");
        let doubled = parse("ä,,b").expect_err("must reject");
        assert!(matches!(doubled, ProjectionError::EmptyTerm { position: 3 }), "got {doubled:?}");
    }

    #[test]
    fn ascii_whitespace_around_a_name_is_trimmed_and_unicode_whitespace_is_not() {
        pretty_assertions::assert_eq!(
            parse("\u{0C}a\t,b ").expect("valid projection list"),
            parse("a,b").expect("valid projection list")
        );
        let error = parse("\u{A0}a").expect_err("must reject");
        assert!(matches!(error, ProjectionError::InvalidAttributeName(_)), "got {error:?}");
    }

    #[test]
    fn a_blank_term_is_refused_where_it_starts() {
        let between = parse("a, ,b").expect_err("must reject");
        assert!(matches!(between, ProjectionError::EmptyTerm { position: 2 }), "got {between:?}");
        let inside = parse("a{ ").expect_err("must reject");
        assert!(matches!(inside, ProjectionError::EmptyTerm { position: 2 }), "got {inside:?}");
    }

    #[test]
    fn a_group_after_a_group_is_refused_at_the_brace_it_leaves_open() {
        let inside = parse("a{b{c}{d}}").expect_err("must reject");
        assert!(matches!(inside, ProjectionError::UnbalancedBrace { position: 1 }), "got {inside:?}");
        let top = parse("a{b}{c}").expect_err("must reject");
        assert!(matches!(top, ProjectionError::UnbalancedBrace { position: 4 }), "got {top:?}");
    }

    #[test]
    fn whitespace_after_a_group_is_trailing_content() {
        let error = parse("a{b} ,c").expect_err("must reject");
        assert!(matches!(error, ProjectionError::UnbalancedBrace { position: 4 }), "got {error:?}");
    }

    #[test]
    fn nesting_has_no_depth_bound() {
        let text = "a{".repeat(128) + "b" + &"}".repeat(128);
        let terms = parse(&text).expect("valid projection list");
        pretty_assertions::assert_eq!(terms[0].linked_depth(), 128);
    }
}
