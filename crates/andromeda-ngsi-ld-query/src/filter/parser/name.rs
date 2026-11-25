//! A name of a `q` filter: an attribute, a sub-attribute, a member, a hop's Relationship or an entity type.
//!
//! Unquoted, a name is exactly the clause 4.9 `AttrName` or `EntityType`, a letter followed by letters, digits and `_`. Every other name, a prefixed name (clause 4.6.2) or an absolute IRI, is written as the grammar's own `quotedStr`, whose value is the name. A `q` has to carry such names, since clause 5.5.7 renders a name no term abbreviates as a Fully Qualified Name and clause 4.3.6.6 compacts a forwarded query into compact IRIs, and a JSON string holds any of them exactly. The quotes only delimit, so every name is then expanded alike. The production that reads a name validates it.

use crate::{
    filter::{
        filter_error::FilterError,
        parser::quoted_string::{decode_quoted_string, quoted_literal},
    },
    query_text::grammar_fault::GrammarFault,
};
use andromeda_ngsi_ld_lexicon::name::unprefixed::is_unprefixed_name;
use std::{borrow::Cow, str};
use winnow::{
    error::ParserError,
    stream::{LocatingSlice, Location, Stream},
};

/// What an unquoted colon directly after a name means where the name stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColonAfter {
    /// Nothing the grammar derives, so the name is refused as one that goes in double quotes.
    Refused,
    /// The end of a hop's type list, which the hop reads.
    EndsTypeList,
}

/// Reads one name: an unquoted `AttrName`, or a `quotedStr` whose resolved value is the name.
///
/// Backtracks without consuming when neither opens at the start.
///
/// # Errors
///
/// Refuses with the refusals of [`quoted_literal`] and [`decode_quoted_string`] for a quoted name, and, where `colon` is [`ColonAfter::Refused`], with [`FilterError::UnquotedColonName`] for an unquoted name directly followed by a colon.
fn name<'i>(input: &mut LocatingSlice<&'i str>, colon: ColonAfter) -> Result<Cow<'i, str>, GrammarFault<FilterError>> {
    if input.peek_token() == Some('"') {
        let literal = quoted_literal(input)?;
        return decode_quoted_string(literal.body, literal.start).map(Cow::Owned).map_err(GrammarFault::Refused);
    }
    let start = input.current_token_start();
    let extent = unquoted_extent(input.peek_finish());
    if extent == 0 {
        return Err(ParserError::from_input(input));
    }
    let text = input.next_slice(extent);
    if colon == ColonAfter::Refused && input.peek_token() == Some(':') {
        return Err(GrammarFault::Refused(FilterError::UnquotedColonName { position: start }));
    }
    Ok(Cow::Borrowed(text))
}

/// Reads a name of a path, which no unquoted colon may follow.
///
/// # Errors
///
/// The refusals of [`name`] where a colon after the name is [`ColonAfter::Refused`].
pub(super) fn path_name<'i>(input: &mut LocatingSlice<&'i str>) -> Result<Cow<'i, str>, GrammarFault<FilterError>> {
    name(input, ColonAfter::Refused)
}

/// Reads a name at the head of a hop or in its type list, where an unquoted colon after it ends the list.
///
/// # Errors
///
/// The refusals of [`name`] for a quoted name.
pub(super) fn list_name<'i>(input: &mut LocatingSlice<&'i str>) -> Result<Cow<'i, str>, GrammarFault<FilterError>> {
    name(input, ColonAfter::EndsTypeList)
}

/// The byte length of the unquoted name that opens at the start of `text`, zero when none opens there.
#[must_use]
pub(super) fn unquoted_extent(text: &str) -> usize {
    let mut characters = text.char_indices();
    match characters.next() {
        Some((_, first)) if opens(first) => characters.find(|(_, character)| !continues(*character)).map_or(text.len(), |(index, _)| index),
        Some(_) | None => 0,
    }
}

/// Whether `character` opens an unquoted name, as a Unicode letter does.
fn opens(character: char) -> bool {
    let mut buffer = [0; 4];
    is_unprefixed_name(character.encode_utf8(&mut buffer))
}

/// Whether `character` continues an unquoted name, as a Unicode letter, a digit and `_` do.
fn continues(character: char) -> bool {
    let mut buffer = [b'a'; 5];
    let length = character.encode_utf8(&mut buffer[1..]).len();
    str::from_utf8(&buffer[..=length]).is_ok_and(is_unprefixed_name)
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            filter_error::FilterError,
            parser::name::{ColonAfter, name, unquoted_extent},
        },
        query_text::grammar_fault::GrammarFault,
    };
    use std::borrow::Cow;
    use winnow::stream::{LocatingSlice, Stream};

    /// Reads a name from `text` where a colon after it is `colon`, returning the result and what stays unread.
    fn read(text: &str, colon: ColonAfter) -> (Result<Cow<'_, str>, GrammarFault<FilterError>>, &str) {
        let mut input = LocatingSlice::new(text);
        let result = name(&mut input, colon);
        (result, input.peek_finish())
    }

    #[test]
    fn an_unquoted_name_is_a_letter_followed_by_letters_digits_and_underscores() {
        for (text, extent) in [
            ("speed>50", 5),
            ("x_1.a", 3),
            ("München==1", 8),
            ("a-b", 1),
            ("a/b", 1),
            ("a#b", 1),
            ("a:b", 1),
            ("true==1", 4),
        ] {
            pretty_assertions::assert_eq!(unquoted_extent(text), extent, "{text}");
        }
        for text in ["_a", ":a", "/a", "1a", "-a", "\"a\"", ""] {
            pretty_assertions::assert_eq!(unquoted_extent(text), 0, "{text}");
        }
    }

    #[test]
    fn a_quoted_name_is_the_value_of_its_string_with_its_escapes_resolved() {
        for (text, value, rest) in [
            (r#""ex:speed">50"#, "ex:speed", ">50"),
            (r#""https://example.org/speed".a"#, "https://example.org/speed", ".a"),
            (r#""speed">50"#, "speed", ">50"),
        ] {
            let (result, unread) = read(text, ColonAfter::Refused);
            pretty_assertions::assert_eq!(result.expect("a quoted name").as_ref(), value, "{text}");
            pretty_assertions::assert_eq!(unread, rest, "{text}");
        }
    }

    #[test]
    fn an_unquoted_name_directly_before_a_colon_is_refused_unless_the_colon_ends_a_type_list() {
        let (result, _) = read("ex:speed>50", ColonAfter::Refused);
        assert!(
            matches!(result, Err(GrammarFault::Refused(FilterError::UnquotedColonName { position: 0 }))),
            "got {result:?}"
        );
        let (result, rest) = read("Device:humidity", ColonAfter::EndsTypeList);
        pretty_assertions::assert_eq!(result.expect("a type").as_ref(), "Device");
        pretty_assertions::assert_eq!(rest, ":humidity");
        let (result, rest) = read(r#""ex:Device":humidity"#, ColonAfter::Refused);
        pretty_assertions::assert_eq!(result.expect("a quoted name").as_ref(), "ex:Device");
        pretty_assertions::assert_eq!(rest, ":humidity");
    }

    #[test]
    fn no_name_opens_at_a_character_no_attr_name_opens_with() {
        for text in ["_a", ":a", "/a", "1a", "==1", ""] {
            let (result, rest) = read(text, ColonAfter::Refused);
            assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "{text}: got {result:?}");
            pretty_assertions::assert_eq!(rest, text);
        }
    }

    #[test]
    fn a_quoted_name_left_open_is_refused() {
        let (result, _) = read(r#""ex:speed>50"#, ColonAfter::Refused);
        assert!(matches!(result, Err(GrammarFault::Refused(FilterError::UnterminatedString))), "got {result:?}");
    }
}
