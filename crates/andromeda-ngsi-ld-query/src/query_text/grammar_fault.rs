//! The error a parser over query text carries while it reads.

use winnow::{
    error::{FromExternalError, ParserError},
    stream::{LocatingSlice, Location},
};

/// Why a parser over query text stopped while it reads.
///
/// `E` is the error of the language being read.
#[derive(Debug)]
pub(crate) enum GrammarFault<E> {
    /// An alternative did not match, so another may be tried.
    Backtrack {
        /// Byte offset in the text where the alternative did not match.
        position: usize,
    },
    /// The text is outside the language for the reason `E` gives. No alternative is tried after it.
    Refused(E),
}

impl<E> GrammarFault<E> {
    /// The language's error for a fault that reached an entry point: a refusal as decided, a backtrack as `backtrack` gives it for its position.
    pub(crate) fn finish(self, backtrack: impl FnOnce(usize) -> E) -> E {
        match self {
            GrammarFault::Backtrack { position } => backtrack(position),
            GrammarFault::Refused(error) => error,
        }
    }
}

impl<'i, E> ParserError<LocatingSlice<&'i str>> for GrammarFault<E> {
    type Inner = Self;

    fn from_input(input: &LocatingSlice<&'i str>) -> Self {
        GrammarFault::Backtrack {
            position: input.current_token_start(),
        }
    }

    fn is_backtrack(&self) -> bool {
        match self {
            GrammarFault::Backtrack { .. } => true,
            GrammarFault::Refused(_) => false,
        }
    }

    fn into_inner(self) -> Result<Self::Inner, Self> {
        Ok(self)
    }
}

impl<'i, E> FromExternalError<LocatingSlice<&'i str>, E> for GrammarFault<E> {
    fn from_external_error(_input: &LocatingSlice<&'i str>, error: E) -> Self {
        GrammarFault::Refused(error)
    }
}

#[cfg(test)]
mod tests {
    use crate::{query_text::grammar_fault::GrammarFault, type_selection::type_selection_error::TypeSelectionError};
    use winnow::{Parser, stream::LocatingSlice};

    #[test]
    fn a_refusal_passes_through_unchanged() {
        let error = GrammarFault::Refused(TypeSelectionError::Empty).finish(|position| TypeSelectionError::UnbalancedParenthesis { position });
        assert!(matches!(error, TypeSelectionError::Empty));
    }

    #[test]
    fn a_backtrack_finishes_as_its_language_gives_it() {
        let error = GrammarFault::<TypeSelectionError>::Backtrack { position: 3 }.finish(|position| TypeSelectionError::UnbalancedParenthesis { position });
        assert!(matches!(error, TypeSelectionError::UnbalancedParenthesis { position: 3 }));
    }

    #[test]
    fn a_backtrack_carries_its_byte_offset() {
        let mut input = LocatingSlice::new("é@");
        let result: Result<(char, char), GrammarFault<TypeSelectionError>> = ('é', 'x').parse_next(&mut input);
        assert!(matches!(result, Err(GrammarFault::Backtrack { position: 2 })));
    }

    #[test]
    fn a_validated_value_arrives_refused() {
        let mut input = LocatingSlice::new("a");
        let result: Result<char, GrammarFault<TypeSelectionError>> = 'a'.try_map(|_| Err::<char, _>(TypeSelectionError::Empty)).parse_next(&mut input);
        assert!(matches!(result, Err(GrammarFault::Refused(TypeSelectionError::Empty))));
    }
}
