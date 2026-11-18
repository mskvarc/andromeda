//! The sets of characters a query language reads as whitespace.

use crate::query_text::grammar_fault::GrammarFault;
use winnow::{Parser, stream::LocatingSlice, token::take_while};

/// A set of characters a query language reads as whitespace, named after its members.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Whitespace {
    /// Every character `char::is_whitespace` admits (the Unicode `White_Space` property). The disjunctive form skips it around every item, parenthesis and operator, and type selection trims its text with it.
    Unicode,
    /// Space, tab, line feed and carriage return, the RFC 8259 section 2 `ws` set. The `q` filter skips it between any two of its words, operators and punctuation marks.
    Json,
    /// Space, tab, line feed, form feed and carriage return, the set `u8::is_ascii_whitespace` admits. Projection trims the names of its terms with it.
    Ascii,
}

impl Whitespace {
    /// Whether the set holds `character`.
    pub(crate) const fn contains(self, character: char) -> bool {
        match self {
            Whitespace::Unicode => character.is_whitespace(),
            Whitespace::Json => matches!(character, ' ' | '\t' | '\n' | '\r'),
            Whitespace::Ascii => character.is_ascii_whitespace(),
        }
    }

    /// `text` without the run of the set at either end.
    pub(crate) fn trim(self, text: &str) -> &str {
        text.trim_matches(|character| self.contains(character))
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
    fn the_json_set_is_the_four_rfc_8259_characters() {
        for space in [' ', '\t', '\n', '\r'] {
            assert!(Whitespace::Json.contains(space), "{space:?}");
        }
        for other in ['\u{0C}', '\u{A0}'] {
            assert!(!Whitespace::Json.contains(other), "{other:?}");
        }
    }

    #[test]
    fn the_ascii_set_takes_form_feed() {
        assert!(Whitespace::Ascii.contains('\u{0C}'));
        assert!(!Whitespace::Ascii.contains('\u{A0}'));
    }

    #[test]
    fn trimming_removes_the_set_from_both_ends_only() {
        pretty_assertions::assert_eq!(Whitespace::Ascii.trim("\u{0C} a\u{A0}b \t"), "a\u{A0}b");
        pretty_assertions::assert_eq!(Whitespace::Unicode.trim("\u{2003}a b\u{3000}"), "a b");
    }

    #[test]
    fn a_run_of_the_set_is_skipped_up_to_the_first_other_character() {
        let mut input = LocatingSlice::new(" \u{2003}a");
        let skipped: Result<(), GrammarFault<TypeSelectionError>> = Whitespace::Unicode.skip().parse_next(&mut input);
        assert!(skipped.is_ok());
        assert_eq!(input.current_token_start(), 4);
    }
}
