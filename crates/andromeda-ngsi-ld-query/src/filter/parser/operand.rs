//! The operand of a clause 4.9 Query Term.
//!
//! ```text
//! ComparableValue   = Number / quotedStr / dateTime / date / time
//! ValueList         = Value 1*(%x2C Value)
//! Range             = ComparableValue dots ComparableValue
//! CompEqualityValue = OtherValue / ValueList / Range / URI
//! ```
//!
//! What separates these productions from the attribute path is that they are
//! read entirely from literals: the operand's own lexical form decides which
//! domain it belongs to, because the grammar gives it no type tag.

use crate::{
    filter::{
        equality_value::EqualityValue,
        expectation::{Expectation, Found},
        filter_error::FilterError,
        operator_spelling::RANGE,
        parser::{
            expected,
            quoted_string::{decode_quoted_string, quoted_literal},
            time_value_extent::time_value_extent,
        },
        quoting::Quoting,
        scalar::Scalar,
    },
    pattern::{Pattern, bare_operand::bare_pattern_length},
    query_text::{grammar_fault::GrammarFault, whitespace::Whitespace},
};
use andromeda_ngsi_ld_number::JsonNumber;
use winnow::{
    Parser,
    combinator::{preceded, repeat},
    error::ParserError,
    stream::{LocatingSlice, Location, Stream},
};

/// Which characters a bare operand word runs through, chosen by where the word stands.
///
/// Clause 4.9 lets a bare `URI` stand only in a `CompEqualityValue`, after `==`
/// or `!=` and through the `,` and `..` that continue it. Everywhere else a `.`
/// ends the word, so a dotted word splits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BareWordExtent {
    /// The extent of a word, which ends at a `.`: the operand of an ordering operator.
    Word,
    /// The extent of a `URI`, which runs through a `.` unless a second one follows: every member of an equality value.
    Uri,
}

impl BareWordExtent {
    /// The byte length of the bare word that opens at the start of `text`.
    fn of(self, text: &str) -> usize {
        match self {
            BareWordExtent::Word => word_extent(text),
            BareWordExtent::Uri => uri_extent(text),
        }
    }
}

/// Whether a bare operand word opens at the start of `text`.
///
/// A word opens at an alphanumeric character that is no ASCII digit, at `_`, `:` or `/`, and at a `*` that a `-` follows. A digit run opens a number or a time value instead.
fn opens_operand_word(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(|first| {
        (first.is_alphanumeric() && !first.is_ascii_digit()) || matches!(first, '_' | ':' | '/') || (first == '*' && characters.as_str().starts_with('-'))
    })
}

/// The byte length of the bare operand word that opens at the start of `text` where an ordering operand stands.
///
/// The word is the longest run of alphanumeric characters and `_`, `:`, `/`, `-` and `#`, and of a `*` that a `-` precedes or follows. Clause 4.9 derives no such word after an ordering operator, so it is read whole and refused as the one word it is.
fn word_extent(text: &str) -> usize {
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

/// The byte length of the bare `URI` that opens at the start of `text` (RFC 3986 section 2).
///
/// The URI characters that clause 4.9 also uses as query syntax (`;`, `|`, `(`, `)`, `,`, `[`, `]`) keep their query meaning, and a `.` that opens `..` stays the range separator, so a `.` at the end of the text belongs to the word.
#[must_use]
pub(in crate::filter) fn uri_extent(text: &str) -> usize {
    extent_while(text, |current, following| match current {
        '.' => !following.starts_with('.'),
        other => {
            other.is_alphanumeric()
                || matches!(
                    other,
                    '_' | ':' | '/' | '-' | '#' | '~' | '%' | '?' | '@' | '!' | '$' | '&' | '\'' | '*' | '+' | '='
                )
        }
    })
}

/// The byte length of the number that opens at the start of `text`.
///
/// A `-` belongs to it only before an ASCII digit. The integer digits may be followed by a `.` and its digits, only when a digit follows the `.`, so that `1..5` stays a range, and then by an `e` or `E`, an optional sign and digits that may be none, which `JsonNumber` then judges.
#[must_use]
fn number_extent(text: &str) -> usize {
    let unsigned = text
        .strip_prefix('-')
        .filter(|rest| rest.starts_with(|character: char| character.is_ascii_digit()))
        .unwrap_or(text);
    let after_integer = skip_digits(unsigned);
    let after_fraction = after_integer
        .strip_prefix('.')
        .filter(|rest| rest.starts_with(|character: char| character.is_ascii_digit()))
        .map_or(after_integer, skip_digits);
    let after_exponent = after_fraction
        .strip_prefix(['e', 'E'])
        .map_or(after_fraction, |rest| skip_digits(rest.strip_prefix(['+', '-']).unwrap_or(rest)));
    text.len() - after_exponent.len()
}

/// `text` without its leading ASCII digits.
fn skip_digits(text: &str) -> &str {
    text.trim_start_matches(|character: char| character.is_ascii_digit())
}

/// Reads a scalar operand whose bare words run through `extent`: a quoted literal, a number, a time value, a keyword or a bare `URI`.
///
/// Backtracks without consuming when no operand opens at the start. A quoted literal is the one position whose escapes resolve, because clause 4.9 `quotedStr` is the IETF RFC 8259 section 7 `String`. The literal arrives typed: a quoted one is a string whatever it holds, and only a bare one is a time value or a `URI`.
fn scalar<'i>(extent: BareWordExtent) -> impl Parser<LocatingSlice<&'i str>, Scalar, GrammarFault<FilterError>> {
    move |input: &mut LocatingSlice<&'i str>| -> Result<Scalar, GrammarFault<FilterError>> {
        let text = input.peek_finish();
        let mut characters = text.chars();
        match (characters.next(), characters.next()) {
            (Some('"'), _) => quoted_scalar(input),
            (Some('-'), Some(digit)) if digit.is_ascii_digit() => number(input),
            (Some(first), _) if first.is_ascii_digit() => digit_run(input),
            (Some(_), _) if opens_operand_word(text) => bare_scalar(input, extent),
            (Some(_) | None, _) => Err(ParserError::from_input(input)),
        }
    }
}

/// Reads a quoted literal as a string.
fn quoted_scalar(input: &mut LocatingSlice<&str>) -> Result<Scalar, GrammarFault<FilterError>> {
    let literal = quoted_literal(input)?;
    let decoded = decode_quoted_string(literal.body, literal.start).map_err(GrammarFault::Refused)?;
    Scalar::from_literal(&decoded, Quoting::Quoted).map_err(GrammarFault::Refused)
}

/// Reads the number that opens at the start.
fn number(input: &mut LocatingSlice<&str>) -> Result<Scalar, GrammarFault<FilterError>> {
    let extent = number_extent(input.peek_finish());
    input
        .next_slice(extent)
        .parse::<JsonNumber>()
        .map(Scalar::Number)
        .map_err(|error| GrammarFault::Refused(FilterError::InvalidNumber(error)))
}

/// Reads what an unsigned digit run opens: a time value when a separator and a digit follow the run, else a number.
fn digit_run(input: &mut LocatingSlice<&str>) -> Result<Scalar, GrammarFault<FilterError>> {
    match time_value_extent(input.peek_finish()) {
        Some(extent) => Scalar::from_literal(input.next_slice(extent), Quoting::Bare).map_err(GrammarFault::Refused),
        None => number(input),
    }
}

/// Reads a bare word, which is a boolean when it is exactly `true` or `false`.
fn bare_scalar(input: &mut LocatingSlice<&str>, extent: BareWordExtent) -> Result<Scalar, GrammarFault<FilterError>> {
    let length = extent.of(input.peek_finish());
    let word = input.next_slice(length);
    match word.parse::<bool>() {
        Ok(value) => Ok(Scalar::Bool(value)),
        Err(_) => Scalar::from_literal(word, Quoting::Bare).map_err(GrammarFault::Refused),
    }
}

/// Refuses an operand that clause 4.9 `ComparableValue` does not derive.
///
/// `ComparableValue = Number / quotedStr / dateTime / date / time` leaves out `true`, `false` and a bare `URI`: those derive only from `OtherValue` and `URI`, which the grammar admits after `==` and `!=` alone. The refusal names the character the operand opened with, at `position`.
fn comparable(operand: Scalar, first: Option<char>, position: usize) -> Result<Scalar, GrammarFault<FilterError>> {
    match operand {
        Scalar::Bool(_) | Scalar::Uri(_) => Err(GrammarFault::Refused(FilterError::Expected {
            expected: Expectation::ComparableValue,
            found: Found::of(first),
            position,
        })),
        Scalar::Str(_) | Scalar::Number(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => Ok(operand),
    }
}

/// Reads a clause 4.9 `ComparableValue`, whose bare words run through `extent`.
pub(super) fn comparable_value<'i>(extent: BareWordExtent) -> impl Parser<LocatingSlice<&'i str>, Scalar, GrammarFault<FilterError>> {
    move |input: &mut LocatingSlice<&'i str>| -> Result<Scalar, GrammarFault<FilterError>> {
        Whitespace::Json.skip::<FilterError>().parse_next(input)?;
        let position = input.current_token_start();
        let first = input.peek_token();
        let operand = expected(Expectation::Value, scalar(extent)).parse_next(input)?;
        comparable(operand, first, position)
    }
}

/// Reads the equality value of a comparison: a scalar, a range or a list.
///
/// Every member reads bare words with the `URI` extent. A range needs `ComparableValue` bounds, so a boolean or a bare `URI` before the range separator is refused at the position it started.
pub(super) fn equality_value(input: &mut LocatingSlice<&str>) -> Result<EqualityValue, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let start = input.current_token_start();
    let first_character = input.peek_token();
    let first = expected(Expectation::Value, scalar(BareWordExtent::Uri)).parse_next(input)?;
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    if input.peek_finish().starts_with(RANGE) {
        let min = comparable(first, first_character, start)?;
        let _separator = input.next_slice(RANGE.len());
        let max = comparable_value(BareWordExtent::Uri).parse_next(input)?;
        return Ok(EqualityValue::Range { min, max });
    }
    let rest: Vec<Scalar> = repeat(
        0..,
        preceded(
            (Whitespace::Json.skip::<FilterError>(), ','),
            expected(Expectation::Value, scalar(BareWordExtent::Uri)),
        ),
    )
    .parse_next(input)?;
    if rest.is_empty() {
        return Ok(EqualityValue::Scalar(first));
    }
    let mut values = vec![first];
    values.extend(rest);
    Ok(EqualityValue::ValueList(values))
}

/// Reads the operand of `~=` and `!~=`, a pattern written bare or in quotes.
///
/// The operand is compiled here rather than at evaluation time, so a malformed regular expression is a rejected request instead of a filter that quietly matches nothing. Clause 4.9 `QueryTerm =/ Attribute patternOp RegExp` makes `RegExp` its own terminal, "a regular expression as mandated by IEEE 1003.2", with no quotes around it and no `quotedStr` escapes. A pattern that opens with `"` is a quoted one, which ends at its closing quote and keeps its body as written, because the backslash is one of its metacharacters and resolving RFC 8259 escapes would refuse `^example\.com$`. Any other pattern is bare and ends where [`bare_pattern_length`] says.
pub(super) fn pattern_operand(input: &mut LocatingSlice<&str>) -> Result<Pattern, GrammarFault<FilterError>> {
    expected(Expectation::RegularExpression, pattern_text)
        .try_map(|text: &str| text.parse::<Pattern>().map_err(FilterError::from))
        .parse_next(input)
}

/// Reads the text of a pattern operand, the body of a quoted one or the whole of a bare one.
///
/// Backtracks without consuming when a bare pattern holds no character, and refuses a quoted pattern without its closing quote.
fn pattern_text<'i>(input: &mut LocatingSlice<&'i str>) -> Result<&'i str, GrammarFault<FilterError>> {
    if input.peek_finish().starts_with('"') {
        return quoted_literal(input).map(|literal| literal.body);
    }
    match bare_pattern_length(input.peek_finish()) {
        Ok(0) => Err(ParserError::from_input(input)),
        Ok(length) => Ok(input.next_slice(length)),
        Err(error) => Err(GrammarFault::Refused(FilterError::from(error))),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            Filter,
            equality_value::EqualityValue,
            expectation::{Expectation, Found},
            filter_error::FilterError,
            parser::operand::{BareWordExtent, comparable_value, equality_value, number_extent, pattern_operand, scalar, uri_extent},
            query_term::QueryTerm,
            quoting::Quoting,
            scalar::Scalar,
        },
        name_stage::Written,
        query_text::grammar_fault::GrammarFault,
    };
    use andromeda_ngsi_ld_number::JsonNumber;
    use pretty_assertions::assert_eq;
    use std::fmt::Debug;
    use winnow::{
        Parser,
        stream::{LocatingSlice, Stream},
    };

    /// Reads `text` with `parser`, returning the result and what stays unread.
    fn read<'i, O>(
        text: &'i str,
        mut parser: impl Parser<LocatingSlice<&'i str>, O, GrammarFault<FilterError>>,
    ) -> (Result<O, GrammarFault<FilterError>>, &'i str) {
        let mut input = LocatingSlice::new(text);
        let result = parser.parse_next(&mut input);
        (result, input.peek_finish())
    }

    /// The refusal a read ended in.
    fn refusal<O: Debug>(result: Result<O, GrammarFault<FilterError>>) -> FilterError {
        match result {
            Err(GrammarFault::Refused(error)) => error,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_number_runs_through_its_fraction_and_exponent() {
        pretty_assertions::assert_eq!(number_extent("-1.5e+3x"), 7);
        pretty_assertions::assert_eq!(number_extent("1..5"), 1);
        pretty_assertions::assert_eq!(number_extent("1.x"), 1);
        pretty_assertions::assert_eq!(number_extent("1e"), 2);
        pretty_assertions::assert_eq!(number_extent("12.5..20"), 4);
        pretty_assertions::assert_eq!(number_extent("1E-3,2"), 4);
    }

    #[test]
    fn a_bare_uri_runs_through_a_dot_unless_a_second_dot_follows() {
        pretty_assertions::assert_eq!(uri_extent("urn:a.b..c"), 7);
        pretty_assertions::assert_eq!(uri_extent("http://a.org/x,y"), 14);
        pretty_assertions::assert_eq!(uri_extent("urn:a."), 6);
        pretty_assertions::assert_eq!(uri_extent("https://a.example/x?y=1&z=%20~@!$'*+;"), 36);
    }

    #[test]
    fn a_scalar_reads_each_literal_by_its_form() {
        for (text, expected, rest) in [
            ("-1.5e+3x", Scalar::Number("-1.5e+3".parse().unwrap()), "x"),
            ("12:00:00Z,5", Scalar::from_literal("12:00:00Z", Quoting::Bare).unwrap(), ",5"),
            ("12..20", Scalar::Number(12.into()), "..20"),
            ("true;a", Scalar::Bool(true), ";a"),
            ("false", Scalar::Bool(false), ""),
            ("\"a%20b\"x", Scalar::Str("a%20b".to_owned()), "x"),
            ("urn:a.b..c", Scalar::Uri("urn:a.b".to_owned()), "..c"),
            ("http://a.org/x,y", Scalar::Uri("http://a.org/x".to_owned()), ",y"),
        ] {
            let (result, unread) = read(text, scalar(BareWordExtent::Uri));
            pretty_assertions::assert_eq!(result.unwrap_or_else(|fault| panic!("{text}: {fault:?}")), expected, "{text}");
            pretty_assertions::assert_eq!(unread, rest, "{text}");
        }
    }

    #[test]
    fn a_keyword_is_a_boolean_only_as_a_whole_word() {
        let (result, _) = read("truthy", scalar(BareWordExtent::Uri));
        assert!(matches!(refusal(result), FilterError::BareWord));
        let (result, rest) = read("true.x", scalar(BareWordExtent::Uri));
        assert!(matches!(refusal(result), FilterError::BareWord));
        pretty_assertions::assert_eq!(rest, "");
        let (result, rest) = read("true.x", scalar(BareWordExtent::Word));
        pretty_assertions::assert_eq!(result.expect("a keyword"), Scalar::Bool(true));
        pretty_assertions::assert_eq!(rest, ".x");
    }

    #[test]
    fn a_scalar_backtracks_where_no_operand_opens() {
        for text in ["@", "-x", "*]", ")", "", ".5", "~a"] {
            let (result, rest) = read(text, scalar(BareWordExtent::Uri));
            assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "{text}: got {result:?}");
            pretty_assertions::assert_eq!(rest, text);
        }
    }

    #[test]
    fn a_scalar_refuses_what_its_literal_form_makes_malformed() {
        let (result, _) = read("1e-16384", scalar(BareWordExtent::Uri));
        assert!(matches!(refusal(result), FilterError::InvalidNumber(_)));
        let (result, _) = read("2017-12-24T12:00:00+01:00", scalar(BareWordExtent::Uri));
        assert!(matches!(refusal(result), FilterError::InvalidDateTime(_)));
        let (result, _) = read("2017-13-45", scalar(BareWordExtent::Uri));
        assert!(matches!(refusal(result), FilterError::InvalidDate(_)));
    }

    #[test]
    fn a_quoted_operand_reports_an_escape_at_its_place_in_the_text() {
        let text = "a==\"\\q\"";
        let mut input = LocatingSlice::new(text);
        let _prefix = input.next_slice(3);
        let error = refusal(scalar(BareWordExtent::Uri).parse_next(&mut input));
        assert!(matches!(error, FilterError::InvalidStringEscape { escape: 'q', position: 4 }), "got {error:?}");
    }

    #[test]
    fn an_equality_value_is_a_scalar_a_range_or_a_list() {
        let number = |value: i32| Scalar::Number(JsonNumber::from(value));
        for (text, expected) in [
            ("1", EqualityValue::Scalar(number(1))),
            (
                "1..5",
                EqualityValue::Range {
                    min: number(1),
                    max: number(5),
                },
            ),
            (
                " 1 .. 5 ",
                EqualityValue::Range {
                    min: number(1),
                    max: number(5),
                },
            ),
            ("1,2", EqualityValue::ValueList(vec![number(1), number(2)])),
            (" 1 , 2", EqualityValue::ValueList(vec![number(1), number(2)])),
            (
                "12:00:00Z,5",
                EqualityValue::ValueList(vec![Scalar::from_literal("12:00:00Z", Quoting::Bare).unwrap(), number(5)]),
            ),
            (
                "2017-12-24..2017-12-25",
                EqualityValue::Range {
                    min: Scalar::from_literal("2017-12-24", Quoting::Bare).unwrap(),
                    max: Scalar::from_literal("2017-12-25", Quoting::Bare).unwrap(),
                },
            ),
            (
                "http://a.org/x,http://b.org/y;q",
                EqualityValue::ValueList(vec![Scalar::Uri("http://a.org/x".to_owned()), Scalar::Uri("http://b.org/y".to_owned())]),
            ),
            ("urn:a.b", EqualityValue::Scalar(Scalar::Uri("urn:a.b".to_owned()))),
        ] {
            let (result, _) = read(text, equality_value);
            pretty_assertions::assert_eq!(result.unwrap_or_else(|fault| panic!("{text}: {fault:?}")), expected, "{text}");
        }
    }

    #[test]
    fn an_equality_value_leaves_what_follows_it_unread() {
        let (result, rest) = read("http://a.org/x,http://b.org/y;q.r==1", equality_value);
        assert!(result.is_ok());
        pretty_assertions::assert_eq!(rest, ";q.r==1");
    }

    #[test]
    fn a_range_bound_that_is_a_boolean_or_a_uri_is_refused_where_the_operand_started() {
        for (text, character, position) in [
            ("urn:a.b..urn:c.d", 'u', 0),
            ("true..1", 't', 0),
            (" false..1", 'f', 1),
            ("1..true", 't', 3),
            ("1..urn:a", 'u', 3),
        ] {
            let (result, _) = read(text, equality_value);
            let error = refusal(result);
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::ComparableValue, found: Found::Character(found), position: at } if found == character && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn an_equality_value_missing_an_operand_is_refused_where_one_was_required() {
        for (text, found, position) in [
            ("@", Found::Character('@'), 0),
            ("", Found::EndOfInput, 0),
            (" -x", Found::Character('-'), 1),
            ("1,", Found::EndOfInput, 2),
            ("1, @", Found::Character('@'), 3),
        ] {
            let (result, _) = read(text, equality_value);
            let error = refusal(result);
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::Value, found: refused, position: at } if refused == found && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_comparable_value_with_the_word_extent_stops_a_bare_word_at_a_dot_and_refuses_booleans_and_uris() {
        let (result, rest) = read("a.b", comparable_value(BareWordExtent::Word));
        assert!(matches!(refusal(result), FilterError::BareWord));
        pretty_assertions::assert_eq!(rest, ".b");
        let (result, _) = read("true", comparable_value(BareWordExtent::Word));
        let error = refusal(result);
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::ComparableValue,
                    found: Found::Character('t'),
                    position: 0
                }
            ),
            "got {error:?}"
        );
        let (result, _) = read(" urn:ngsi-ld:T:a", comparable_value(BareWordExtent::Word));
        let error = refusal(result);
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::ComparableValue,
                    found: Found::Character('u'),
                    position: 1
                }
            ),
            "got {error:?}"
        );
        let (result, rest) = read("2024-01-01,x", comparable_value(BareWordExtent::Word));
        assert!(result.is_ok());
        pretty_assertions::assert_eq!(rest, ",x");
    }

    #[test]
    fn a_pattern_operand_is_quoted_or_bare_and_keeps_its_backslashes() {
        for (text, pattern, rest) in [
            (r#" "^example\.com$";x"#, r"^example\.com$", ";x"),
            (r#""a|b;c")"#, "a|b;c", ")"),
            (r"^example\.com$;x", r"^example\.com$", ";x"),
            ("abc.d", "abc.d", ""),
            ("^Mer(c|z)edes$|x", "^Mer(c|z)edes$", "|x"),
            ("^M ;x", "^M", " ;x"),
            ("1", "1", ""),
            ("true", "true", ""),
            (" -)", "-", ")"),
            ("[\"]Merc[\"]", "[\"]Merc[\"]", ""),
        ] {
            let (result, unread) = read(text, pattern_operand);
            pretty_assertions::assert_eq!(result.unwrap_or_else(|fault| panic!("{text}: {fault:?}")).as_str(), pattern, "{text}");
            pretty_assertions::assert_eq!(unread, rest, "{text}");
        }
    }

    #[test]
    fn a_pattern_operand_that_holds_no_character_is_refused_where_it_was_required() {
        for (text, found, position) in [
            ("", Found::EndOfInput, 0),
            (" ;x", Found::Character(';'), 1),
            ("|x", Found::Character('|'), 0),
            (")", Found::Character(')'), 0),
        ] {
            let (result, _) = read(text, pattern_operand);
            let error = refusal(result);
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::RegularExpression, found: refused, position: at } if refused == found && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_quoted_pattern_operand_without_its_closing_quote_is_refused() {
        let (result, _) = read("\"Merc;speed>5", pattern_operand);
        assert!(matches!(refusal(result), FilterError::UnterminatedString));
    }

    /// The equality operand of a filter holding one query term.
    fn equality_operand(q: &str) -> EqualityValue {
        let Filter::Term(QueryTerm::Equality { value, .. }) = q.parse::<Filter<Written>>().expect("parse q") else {
            panic!("a single equality query term");
        };
        value
    }

    /// The filter keeps the operand the client wrote, a URI included, so a
    /// stored or forwarded filter reads back as written (clause 5.2.12); the
    /// reading holds the spellings it is compared in.
    #[test]
    fn a_lone_uri_operand_parses_as_written() {
        assert_eq!(
            equality_operand("rel==\"HTTP://Example.com/a\""),
            EqualityValue::Scalar(Scalar::Str("HTTP://Example.com/a".to_owned()))
        );
        assert_eq!(
            equality_operand("brandName==\"Mercedes\""),
            EqualityValue::Scalar(Scalar::Str("Mercedes".to_owned()))
        );
    }

    /// Clause 4.9 `ComparableValue = Number / quotedStr / dateTime / date /
    /// time`: a boolean and a bare word derive only from `OtherValue` and
    /// `URI`, which follow `==` and `!=` alone.
    #[test]
    fn a_boolean_or_a_bare_word_after_an_ordering_operator_or_in_a_range_is_refused() {
        for q in [
            "b>true",
            "b<=false",
            "b>=true",
            "b<false",
            "ref>urn:ngsi-ld:Target:a",
            "n==true..false",
            "n==a..b",
            "n==1..true",
        ] {
            assert!(q.parse::<Filter<Written>>().is_err(), "{q} is outside the grammar");
        }
    }

    /// Every `ComparableValue` alternative still stands after an ordering
    /// operator and in a range, and `OtherValue` still stands after `==`.
    #[test]
    fn every_comparable_value_after_an_ordering_operator_or_in_a_range_is_accepted() {
        for q in [
            "n>1",
            "n<\"text\"",
            "n>=2024-01-01T00:00:00Z",
            "n<=2024-01-01",
            "n>12:00:00Z",
            "n==1..2",
            "n==\"a\"..\"b\"",
            "b==true",
            "b!=false",
        ] {
            assert!(q.parse::<Filter<Written>>().is_ok(), "{q} is inside the grammar");
        }
    }
}
