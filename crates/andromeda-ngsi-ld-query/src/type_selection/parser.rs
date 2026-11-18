//! Parser for the NGSI-LD Entity Type Selection Language (clause 4.17).
//!
//! The grammar is flat:
//!
//! ```text
//! EntityTypes  = OrEntityType *(orOp OrEntityType)
//! OrEntityType = "(" EntityType *(";" EntityType) ")" / EntityType
//! orOp         = "|" / ","
//! ```
//!
//! A conjunction exists only inside one pair of parentheses around plain type
//! names, so `Home;Vehicle`, `((Home;Vehicle))` and `(Building|House)` are all
//! outside the language and refused.

use crate::{
    query_text::{
        disjunctive_form::{delimits_item, disjunctive_form},
        grammar_fault::GrammarFault,
        whitespace::Whitespace,
    },
    type_selection::{type_selection_error::TypeSelectionError, written_type_selection::WrittenTypeSelection},
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use std::ops::Range;
use winnow::{Parser, stream::LocatingSlice, token::take_while};

/// Parses the text of a type selection into a [`WrittenTypeSelection`].
///
/// # Errors
///
/// Returns [`TypeSelectionError::Empty`] for blank input,
/// [`TypeSelectionError::UnexpectedEnd`] and [`TypeSelectionError::UnbalancedParenthesis`]
/// for a selection that ends early or leaves a parenthesis open,
/// [`TypeSelectionError::UnexpectedCharacter`] for a character the clause 4.17 grammar
/// does not admit at its position, and [`TypeSelectionError::InvalidName`]
/// when a type-name production yields no valid clause 4.6.2 name.
pub(super) fn parse(input: &str) -> Result<WrittenTypeSelection, TypeSelectionError> {
    let trimmed = input.trim_matches(|character| Whitespace::Unicode.contains(character));
    if trimmed.is_empty() {
        return Err(TypeSelectionError::Empty);
    }
    if trimmed == "*" {
        return Ok(WrittenTypeSelection::Wildcard);
    }
    disjunctive_form(trimmed, type_name).map(disjunction)
}

/// The selection of the alternatives of a form: the only alternative itself, or their disjunction.
fn disjunction(alternatives: Vec<Vec<EntityTypeName>>) -> WrittenTypeSelection {
    let alternatives = alternatives.into_iter().map(conjunction).collect::<Vec<_>>();
    match <[WrittenTypeSelection; 1]>::try_from(alternatives) {
        Ok([only]) => only,
        Err(several) => WrittenTypeSelection::Or(several),
    }
}

/// The selection of the names of one alternative: the only name itself, or their conjunction.
fn conjunction(names: Vec<EntityTypeName>) -> WrittenTypeSelection {
    match <[EntityTypeName; 1]>::try_from(names) {
        Ok([only]) => WrittenTypeSelection::Type(only),
        Err(several) => WrittenTypeSelection::And(several.into_iter().map(WrittenTypeSelection::Type).collect()),
    }
}

/// Reads one `EntityType`: a clause 4.6.2 name or a URI.
fn type_name(input: &mut LocatingSlice<&str>) -> Result<EntityTypeName, GrammarFault<TypeSelectionError>> {
    take_while(1.., is_type_character)
        .with_span()
        .try_map(|(name, span): (&str, Range<usize>)| {
            name.parse::<EntityTypeName>()
                .map_err(|source| TypeSelectionError::InvalidName { position: span.start, source })
        })
        .parse_next(input)
}

/// Whether `character` may appear in a type name: neither a delimiter of the grammar nor whitespace.
fn is_type_character(character: char) -> bool {
    !delimits_item(character) && !Whitespace::Unicode.contains(character)
}

#[cfg(test)]
mod tests {
    use crate::type_selection::{parser::parse, type_selection_error::TypeSelectionError, written_type_selection::WrittenTypeSelection};

    fn t(name: &str) -> WrittenTypeSelection {
        WrittenTypeSelection::Type(name.parse().expect("valid type name"))
    }

    #[test]
    fn parse_single_type() {
        let type_selection = parse("Vehicle").unwrap();
        pretty_assertions::assert_eq!(type_selection, t("Vehicle"));
    }

    #[test]
    fn parse_or_with_pipe() {
        let type_selection = parse("Building|House").unwrap();
        pretty_assertions::assert_eq!(type_selection, WrittenTypeSelection::Or(vec![t("Building"), t("House"),]));
    }

    #[test]
    fn parse_or_with_comma() {
        let type_selection = parse("Building,House").unwrap();
        pretty_assertions::assert_eq!(type_selection, WrittenTypeSelection::Or(vec![t("Building"), t("House"),]));
    }

    #[test]
    fn parse_parenthesised_conjunction() {
        let type_selection = parse("(Home;Vehicle)").unwrap();
        pretty_assertions::assert_eq!(type_selection, WrittenTypeSelection::And(vec![t("Home"), t("Vehicle"),]));
    }

    #[test]
    fn parse_mixed_and_or_with_grouping() {
        let type_selection = parse("(Home;Vehicle)|Motorhome").unwrap();
        pretty_assertions::assert_eq!(
            type_selection,
            WrittenTypeSelection::Or(vec![WrittenTypeSelection::And(vec![t("Home"), t("Vehicle"),]), t("Motorhome"),])
        );
    }

    #[test]
    fn a_parenthesised_single_name_is_that_name() {
        pretty_assertions::assert_eq!(parse("(Home)").unwrap(), t("Home"));
    }

    #[test]
    fn a_shape_outside_the_clause_4_17_grammar_is_rejected() {
        for raw in [
            "Home;Vehicle",
            "((Home;Vehicle))",
            "(Building|House)",
            "((A;B)|C)",
            "A|B;C",
            "(A;(B|C))",
            "()",
            "A|",
        ] {
            assert!(parse(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn parse_uri_type_name() {
        let type_selection = parse("https://example.com/Vehicle").unwrap();
        pretty_assertions::assert_eq!(type_selection, t("https://example.com/Vehicle"));
    }

    #[test]
    fn a_type_name_outside_the_clause_4_6_2_grammar_is_rejected() {
        // A leaf must satisfy the NGSI-LD name grammar, so a name that is neither
        // a term, a compact IRI, nor an absolute IRI/URI is a parse error rather
        // than a silently accepted `Type` node.
        assert!(matches!(parse("42bad"), Err(TypeSelectionError::InvalidName { .. })));
        assert!(matches!(parse("%%%"), Err(TypeSelectionError::InvalidName { .. })));
    }

    #[test]
    fn error_on_empty_input() {
        let result = parse("");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_whitespace_only() {
        let result = parse("   ");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unbalanced_open_paren() {
        let result = parse("(A;B");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unbalanced_close_paren() {
        let result = parse("(A;B))");
        assert!(result.is_err());
    }

    #[test]
    fn multiple_or_terms() {
        let type_selection = parse("A|B|C").unwrap();
        pretty_assertions::assert_eq!(type_selection, WrittenTypeSelection::Or(vec![t("A"), t("B"), t("C"),]));
    }

    #[test]
    fn unicode_whitespace_separates_every_slot_and_a_position_counts_bytes() {
        pretty_assertions::assert_eq!(
            parse("A\u{2003}|\u{3000}(B ;\u{A0}C)").unwrap(),
            WrittenTypeSelection::Or(vec![t("A"), WrittenTypeSelection::And(vec![t("B"), t("C")])])
        );
        // U+3000 takes three bytes, so the `|` that leaves the group open stands at byte 5.
        assert!(matches!(parse("(A\u{3000}|B)"), Err(TypeSelectionError::UnbalancedParenthesis { position: 5 })));
    }
}
