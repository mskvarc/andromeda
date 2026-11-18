//! The sets of characters a query language reads as whitespace.

use crate::query_text::grammar_fault::GrammarFault;
use winnow::{Parser, stream::LocatingSlice, token::take_while};

/// A set of characters a query language reads as whitespace, named after its members.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Whitespace {
    /// Every character `char::is_whitespace` admits (the Unicode `White_Space` property). The disjunctive form skips it around every item, parenthesis and operator, and type selection trims its text with it.
    Unicode,
}

impl Whitespace {
    /// Whether the set holds `character`.
    pub(crate) const fn contains(self, character: char) -> bool {
        match self {
            Whitespace::Unicode => character.is_whitespace(),
        }
    }

    /// Skips a run of the set, possibly empty.
    pub(crate) fn skip<'i, E>(self) -> impl Parser<LocatingSlice<&'i str>, (), GrammarFault<E>> {
        take_while(0.., move |character: char| self.contains(character)).void()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        query_text::{grammar_fault::GrammarFault, whitespace::Whitespace},
        type_selection::type_selection_error::TypeSelectionError,
    };
    use winnow::{
        Parser,
        stream::{LocatingSlice, Location},
    };

    #[test]
    fn the_unicode_set_takes_every_unicode_space() {
        for space in [' ', '\t', '\u{A0}', '\u{2003}'] {
            assert!(Whitespace::Unicode.contains(space), "{space:?}");
        }
        assert!(!Whitespace::Unicode.contains('a'));
    }

    #[test]
    fn a_run_of_the_set_is_skipped_up_to_the_first_other_character() {
        let mut input = LocatingSlice::new(" \u{2003}a");
        let skipped: Result<(), GrammarFault<TypeSelectionError>> = Whitespace::Unicode.skip().parse_next(&mut input);
        assert!(skipped.is_ok());
        assert_eq!(input.current_token_start(), 4);
    }
}
