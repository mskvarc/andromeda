//! A name or a type of a `q` filter, written as a quoted literal, a time value or a bare word.
//!
//! The same lexical shape stands wherever clause 4.9 puts a name or an entity type, and the production that reads it validates it, so the shape is stated once here.

use crate::{
    filter::{
        filter_error::FilterError,
        parser::{quoted_string::quoted_literal, time_value_extent::time_value_extent},
        quoting::Quoting,
    },
    query_text::grammar_fault::GrammarFault,
};
use winnow::{
    error::ParserError,
    stream::{LocatingSlice, Stream},
};

/// A word as the text spells it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Word<'i> {
    /// The body of a quoted literal with its escapes unresolved, or the bare text.
    pub(super) text: &'i str,
    /// Whether the text was written inside quotation marks.
    pub(super) quoting: Quoting,
}

/// Reads a word: a quoted literal, a digit run that opens a time value, or a bare word with the name extent that is no keyword.
///
/// A bare word ends at a `.`, which clause 4.9 makes the sub-attribute separator. Backtracks without consuming when no word stands at the start: a number, a keyword and a character no word opens with are not words.
pub(super) fn word<'i>(input: &mut LocatingSlice<&'i str>) -> Result<Word<'i>, GrammarFault<FilterError>> {
    let text = input.peek_finish();
    if text.starts_with('"') {
        let literal = quoted_literal(input)?;
        return Ok(Word {
            text: literal.body,
            quoting: Quoting::Quoted,
        });
    }
    let extent = match time_value_extent(text) {
        Some(extent) => extent,
        None if opens_bare_word(text) => name_extent(text),
        None => return Err(ParserError::from_input(input)),
    };
    let start = input.checkpoint();
    let bare = input.next_slice(extent);
    if bare.parse::<bool>().is_ok() {
        input.reset(&start);
        return Err(ParserError::from_input(input));
    }
    Ok(Word {
        text: bare,
        quoting: Quoting::Bare,
    })
}

/// Whether a bare word opens at the start of `text`.
///
/// A word opens at an alphanumeric character that is no ASCII digit, at `_`, `:` or `/`, and at a `*` that a `-` follows. A digit run opens a number or a time value instead.
#[must_use]
pub(super) fn opens_bare_word(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(|first| {
        (first.is_alphanumeric() && !first.is_ascii_digit()) || matches!(first, '_' | ':' | '/') || (first == '*' && characters.as_str().starts_with('-'))
    })
}

/// The byte length of the bare word that opens at the start of `text` where a name stands.
///
/// The word is the longest run of alphanumeric characters and `_`, `:`, `/`, `-` and `#`, and of a `*` that a `-` precedes or follows, the `*` subtag of an RFC 4647 extended language range (`de-*-DE`, `*-GB`, `en-*`). A lone `*` stays the `[*]` every-language marker.
#[must_use]
pub(super) fn name_extent(text: &str) -> usize {
    let mut previous = None;
    extent_while(text, |current, following| {
        let continues = current.is_alphanumeric()
            || matches!(current, '_' | ':' | '/' | '-' | '#')
            || (current == '*' && (following.starts_with('-') || previous == Some('-')));
        previous = Some(current);
        continues
    })
}

/// The byte length of the longest prefix of `text` whose characters all satisfy `continues`, which is given each character with the text after it.
pub(super) fn extent_while(text: &str, mut continues: impl FnMut(char, &str) -> bool) -> usize {
    let mut characters = text.chars();
    let mut extent = 0;
    while let Some(current) = characters.next() {
        if !continues(current, characters.as_str()) {
            break;
        }
        extent += current.len_utf8();
    }
    extent
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            filter_error::FilterError,
            parser::word::{Word, name_extent, opens_bare_word, word},
            quoting::Quoting,
        },
        query_text::grammar_fault::GrammarFault,
    };
    use winnow::{
        Parser,
        stream::{LocatingSlice, Stream},
    };

    #[test]
    fn a_bare_word_where_a_name_stands_ends_at_a_dot() {
        pretty_assertions::assert_eq!(name_extent("https://example.com"), 15);
        pretty_assertions::assert_eq!(name_extent("ngsi-ld:speed_1==1"), 15);
        pretty_assertions::assert_eq!(name_extent("é日本#x.y"), 10);
    }

    #[test]
    fn a_star_joins_a_bare_word_only_beside_a_hyphen() {
        pretty_assertions::assert_eq!(name_extent("de-*-DE]"), 7);
        pretty_assertions::assert_eq!(name_extent("*-GB]"), 4);
        pretty_assertions::assert_eq!(name_extent("en-*]"), 4);
        assert!(opens_bare_word("*-GB]"));
        assert!(!opens_bare_word("*]"));
    }

    #[test]
    fn a_bare_word_opens_at_a_letter_an_underscore_a_colon_or_a_slash_and_not_at_a_digit() {
        for text in ["a", "_a", ":a", "/a", "é"] {
            assert!(opens_bare_word(text), "{text}");
        }
        for text in ["1a", "-a", ".a", "\"a", "@", ""] {
            assert!(!opens_bare_word(text), "{text}");
        }
    }

    fn read(text: &str) -> (Result<Word<'_>, GrammarFault<FilterError>>, &str) {
        let mut input = LocatingSlice::new(text);
        let result = word.parse_next(&mut input);
        (result, input.peek_finish())
    }

    #[test]
    fn true_and_false_are_no_words() {
        let (result, rest) = read("true==1");
        assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "got {result:?}");
        pretty_assertions::assert_eq!(rest, "true==1");
        let (result, _) = read("false");
        assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "got {result:?}");
    }

    #[test]
    fn a_word_that_merely_starts_like_a_keyword_is_a_word() {
        let (result, rest) = read("truthy.x");
        pretty_assertions::assert_eq!(
            result.expect("a bare word"),
            Word {
                text: "truthy",
                quoting: Quoting::Bare
            }
        );
        pretty_assertions::assert_eq!(rest, ".x");
    }

    #[test]
    fn a_number_and_a_character_no_word_opens_with_are_no_words() {
        for text in ["12", "-5", "@a", ".a"] {
            let (result, rest) = read(text);
            assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "{text}: got {result:?}");
            pretty_assertions::assert_eq!(rest, text);
        }
    }

    #[test]
    fn a_quoted_literal_is_a_word_with_its_body_as_written() {
        let (result, rest) = read(r#""a\"b".c"#);
        pretty_assertions::assert_eq!(
            result.expect("a quoted word"),
            Word {
                text: r#"a\"b"#,
                quoting: Quoting::Quoted
            }
        );
        pretty_assertions::assert_eq!(rest, ".c");
    }

    #[test]
    fn a_digit_run_that_opens_a_time_value_is_a_bare_word() {
        let (result, rest) = read("2017-12-24..x");
        pretty_assertions::assert_eq!(
            result.expect("a time value"),
            Word {
                text: "2017-12-24",
                quoting: Quoting::Bare
            }
        );
        pretty_assertions::assert_eq!(rest, "..x");
    }
}
