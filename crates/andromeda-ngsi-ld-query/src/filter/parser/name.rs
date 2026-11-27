//! A name of a `q` filter: an attribute, a sub-attribute, a member, a hop's Relationship or an entity type.
//!
//! A name is written unquoted or as the grammar's own `quotedStr`, whose value is the name. Unquoted, it runs as far as its position admits: through `-`, `:`, `/` and `#` wherever an unquoted colon has no meaning of its own, and through the clause 4.9 `AttrName` alone at the head of a hop's braces and in its type list, where an unquoted colon ends the list. A `q` has to carry prefixed names and absolute IRIs, since clause 5.5.7 renders a name no term abbreviates as a Fully Qualified Name and clause 4.3.6.6 compacts a forwarded query into compact IRIs, and a JSON string holds any of them exactly. Either spelling only delimits, so every name is then expanded alike, and the production that reads a name validates it whichever spelling it came in.

use crate::{
    filter::{
        filter_error::FilterError,
        name_position::NamePosition,
        parser::quoted_string::{decode_quoted_string, quoted_literal},
    },
    query_text::grammar_fault::GrammarFault,
};
use std::borrow::Cow;
use winnow::{
    error::ParserError,
    stream::{LocatingSlice, Stream},
};

/// Reads one name standing at `position`: an unquoted name of that position, or a `quotedStr` whose resolved value is the name.
///
/// Backtracks without consuming when neither opens at the start.
///
/// # Errors
///
/// Refuses with the refusals of [`quoted_literal`] and [`decode_quoted_string`] for a quoted name.
fn name<'i>(input: &mut LocatingSlice<&'i str>, position: NamePosition) -> Result<Cow<'i, str>, GrammarFault<FilterError>> {
    if input.peek_token() == Some('"') {
        let literal = quoted_literal(input)?;
        return decode_quoted_string(literal.body, literal.start).map(Cow::Owned).map_err(GrammarFault::Refused);
    }
    let extent = position.unquoted_extent(input.peek_finish());
    if extent == 0 {
        return Err(ParserError::from_input(input));
    }
    Ok(Cow::Borrowed(input.next_slice(extent)))
}

/// Reads a name standing where an unquoted colon after it means nothing of its own, so an unquoted name runs through its colons.
///
/// # Errors
///
/// The refusals of [`name`] for a quoted name.
pub(super) fn path_name<'i>(input: &mut LocatingSlice<&'i str>) -> Result<Cow<'i, str>, GrammarFault<FilterError>> {
    name(input, NamePosition::Path)
}

/// Reads a name at the head of a hop or in its type list, where an unquoted colon after it ends the list.
///
/// # Errors
///
/// The refusals of [`name`] for a quoted name.
pub(super) fn list_name<'i>(input: &mut LocatingSlice<&'i str>) -> Result<Cow<'i, str>, GrammarFault<FilterError>> {
    name(input, NamePosition::HopHead)
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{filter_error::FilterError, name_position::NamePosition, parser::name::name},
        query_text::grammar_fault::GrammarFault,
    };
    use std::borrow::Cow;
    use winnow::stream::{LocatingSlice, Stream};

    /// Reads a name standing at `position` from `text`, returning the result and what stays unread.
    fn read(text: &str, position: NamePosition) -> (Result<Cow<'_, str>, GrammarFault<FilterError>>, &str) {
        let mut input = LocatingSlice::new(text);
        let result = name(&mut input, position);
        (result, input.peek_finish())
    }

    #[test]
    fn a_quoted_name_is_the_value_of_its_string_with_its_escapes_resolved() {
        for position in [NamePosition::Path, NamePosition::HopHead] {
            for (text, value, rest) in [
                (r#""ex:speed">50"#, "ex:speed", ">50"),
                (r#""https://example.org/speed".a"#, "https://example.org/speed", ".a"),
                (r#""speed">50"#, "speed", ">50"),
                (r#""ex:Device":humidity"#, "ex:Device", ":humidity"),
            ] {
                let (result, unread) = read(text, position);
                pretty_assertions::assert_eq!(result.expect("a quoted name").as_ref(), value, "{text}");
                pretty_assertions::assert_eq!(unread, rest, "{text}");
            }
        }
    }

    #[test]
    fn an_unquoted_name_of_a_path_reads_its_colons_and_stops_at_its_first_dot() {
        for (text, value, rest) in [
            ("ex:speed>50", "ex:speed", ">50"),
            ("urn:xq:weight>1000", "urn:xq:weight", ">1000"),
            ("ex:speed.observedAt", "ex:speed", ".observedAt"),
            ("ex:zip]", "ex:zip", "]"),
            ("ex:sensor{Device:humidity}", "ex:sensor", "{Device:humidity}"),
            ("urn:x:a.b", "urn:x:a", ".b"),
            ("speed-limit>5", "speed-limit", ">5"),
            ("ex:speed;desc", "ex:speed", ";desc"),
        ] {
            let (result, unread) = read(text, NamePosition::Path);
            pretty_assertions::assert_eq!(result.expect("an unquoted name").as_ref(), value, "{text}");
            pretty_assertions::assert_eq!(unread, rest, "{text}");
        }
    }

    #[test]
    fn an_unquoted_name_at_the_head_of_a_hop_stops_at_the_colon_that_ends_the_type_list() {
        for (text, value, rest) in [
            ("ex:humidity}", "ex", ":humidity}"),
            ("Device:ex:humidity}", "Device", ":ex:humidity}"),
            ("humidity}", "humidity", "}"),
            ("A,B:c}", "A", ",B:c}"),
        ] {
            let (result, unread) = read(text, NamePosition::HopHead);
            pretty_assertions::assert_eq!(result.expect("an unquoted name").as_ref(), value, "{text}");
            pretty_assertions::assert_eq!(unread, rest, "{text}");
        }
    }

    #[test]
    fn no_name_opens_at_a_character_no_letter_opens_with() {
        for position in [NamePosition::Path, NamePosition::HopHead] {
            for text in ["_a", ":a", "/a", "#a", "1a", "-a", "==1", ""] {
                let (result, rest) = read(text, position);
                assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "{text}: got {result:?}");
                pretty_assertions::assert_eq!(rest, text);
            }
        }
    }

    #[test]
    fn a_quoted_name_left_open_is_refused() {
        let (result, _) = read(r#""ex:speed>50"#, NamePosition::Path);
        assert!(matches!(result, Err(GrammarFault::Refused(FilterError::UnterminatedString))), "got {result:?}");
    }
}
