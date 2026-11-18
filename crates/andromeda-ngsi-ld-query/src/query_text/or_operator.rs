//! The `orOp` separator the query languages share.

use crate::query_text::grammar_fault::GrammarFault;
use winnow::{Parser, stream::LocatingSlice, token::one_of};

/// The two spellings of `orOp` (ETSI GS CIM 009 v1.9.1 clauses 4.17 and 4.19): `|` and `,`.
pub(crate) const OR_OPERATORS: [char; 2] = ['|', ','];

/// Reads one `orOp`.
pub(crate) fn or_operator<E>(input: &mut LocatingSlice<&str>) -> Result<(), GrammarFault<E>> {
    one_of(OR_OPERATORS).void().parse_next(input)
}

#[cfg(test)]
mod tests {
    use crate::{
        query_text::{grammar_fault::GrammarFault, or_operator::or_operator},
        type_selection::type_selection_error::TypeSelectionError,
    };
    use winnow::stream::{LocatingSlice, Location};

    #[test]
    fn a_pipe_and_a_comma_are_or_operators() {
        for operator in ["|", ","] {
            let mut input = LocatingSlice::new(operator);
            let read: Result<(), GrammarFault<TypeSelectionError>> = or_operator(&mut input);
            assert!(read.is_ok(), "{operator}");
            assert_eq!(input.current_token_start(), 1, "{operator}");
        }
    }

    #[test]
    fn a_semicolon_is_not_an_or_operator() {
        let mut input = LocatingSlice::new(";");
        let read: Result<(), GrammarFault<TypeSelectionError>> = or_operator(&mut input);
        assert!(matches!(read, Err(GrammarFault::Backtrack { position: 0 })));
    }
}
