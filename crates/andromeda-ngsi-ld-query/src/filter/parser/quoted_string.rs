//! The JSON string production the clause 4.9 `quotedStr` terminal delegates to.
//!
//! Clause 4.9 defines `quotedStr = String` and mandates that both `String` and
//! `char` follow the ABNF of IETF RFC 8259 section 7. That grammar admits
//! exactly eight two-character escapes plus `\uXXXX`, forbids an unescaped
//! control character, and spells a code point above the BMP as a UTF-16
//! surrogate pair. Resolving an arbitrary backslash pair to its second
//! character instead would read the escape of é as the letters `u00e9`,
//! and would silently accept escapes the grammar does not define.

use crate::{
    filter::{filter_error::FilterError, unicode_escape_error::UnicodeEscapeError},
    query_text::grammar_fault::GrammarFault,
};
use andromeda_ngsi_ld_number::held_text::admit_held_text;
use std::str::CharIndices;
use winnow::{
    Parser,
    combinator::{alt, delimited, repeat},
    stream::{LocatingSlice, Location},
    token::{any, take_till},
};

/// The number of hexadecimal digits RFC 8259 requires after `\u`.
const UNICODE_ESCAPE_DIGITS: usize = 4;

/// The width of the `\u` introducer that precedes those digits.
const UNICODE_ESCAPE_PREFIX: usize = 2;

/// The lowest code point the RFC 8259 `unescaped` production admits; every
/// code point below it must be written as an escape.
const FIRST_UNESCAPED_CODE_POINT: u32 = 0x20;

/// The first code unit of the UTF-16 high-surrogate range.
const HIGH_SURROGATE_START: u16 = 0xD800;

/// The last code unit of the UTF-16 high-surrogate range.
const HIGH_SURROGATE_END: u16 = 0xDBFF;

/// A double-quoted literal as the text spells it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QuotedLiteral<'i> {
    /// The text between the quotation marks, escapes unresolved.
    pub(super) body: &'i str,
    /// Zero-based byte offset of the opening quotation mark in the text the parser read.
    pub(super) start: usize,
}

/// Reads a double-quoted literal, from its opening quotation mark through its closing one.
///
/// The body is the text between them as written: a backslash takes the character after it, so an escaped quotation mark does not end the literal, and no escape is resolved, because which resolution applies depends on the production the literal stands in. Clause 4.9 puts a `quotedStr` and a `RegExp` in the same lexical shape and gives them different grammars: `quotedStr = String` follows IETF RFC 8259 section 7, while `RegExp` is delegated to IEEE 1003.2, whose own metacharacters include the backslash, so resolving RFC 8259 escapes here would refuse `^example\.com$`. The body is otherwise verbatim: percent-encoding is the HTTP binding's concern (clause 4.9) and is decoded once before a filter reaches the parser, so a second decode would rewrite a URN that legitimately carries `%2F`. Backtracks, consuming nothing, when no quotation mark stands at the start.
///
/// # Errors
///
/// Refuses with [`FilterError::UnterminatedString`] when the text ends before the closing quotation mark, a backslash immediately before the end included.
pub(super) fn quoted_literal<'i>(input: &mut LocatingSlice<&'i str>) -> Result<QuotedLiteral<'i>, GrammarFault<FilterError>> {
    let start = input.current_token_start();
    let body = delimited('"', literal_body, closing_quotation_mark).parse_next(input)?;
    Ok(QuotedLiteral { body, start })
}

/// Reads the text of a literal up to the quotation mark that closes it.
fn literal_body<'i>(input: &mut LocatingSlice<&'i str>) -> Result<&'i str, GrammarFault<FilterError>> {
    repeat::<_, _, (), _, _>(0.., alt((take_till(1.., ['"', '\\']).void(), ('\\', any).void())))
        .take()
        .parse_next(input)
}

/// Reads the quotation mark that closes a literal, refusing its absence.
fn closing_quotation_mark(input: &mut LocatingSlice<&str>) -> Result<char, GrammarFault<FilterError>> {
    '"'.parse_next(input)
        .map_err(|_: GrammarFault<FilterError>| GrammarFault::Refused(FilterError::UnterminatedString))
}

/// Read the body of a quoted literal as the clause 4.9 `quotedStr` production,
/// which IETF RFC 8259 section 7 defines.
///
/// That grammar admits exactly eight two-character escapes plus `\uXXXX`,
/// forbids an unescaped control character, and spells a code point above the
/// BMP as a UTF-16 surrogate pair. Resolving an arbitrary backslash pair to its
/// second character instead would read the escape of é as the letters
/// `u00e9`.
///
/// `literal_start` is the byte offset of the literal's opening quotation mark in
/// the whole text the parser read, so every position in the returned errors is a
/// byte offset in that text: `literal_start + 1` plus the offset inside `body`.
///
/// # Errors
///
/// Returns [`FilterError::InvalidStringEscape`] for a backslash the RFC 8259
/// `escape` production does not admit, [`FilterError::InvalidUnicodeEscape`]
/// for a malformed or unpaired `\uXXXX` form,
/// [`FilterError::UnescapedControlCharacter`] for a raw character below U+0020,
/// [`FilterError::UnterminatedString`] for a trailing lone backslash, and
/// [`FilterError::UnholdableCharacter`] for a literal whose escapes spell a character
/// the broker cannot hold.
pub(super) fn decode_quoted_string(body: &str, literal_start: usize) -> Result<String, FilterError> {
    let body_start = literal_start + 1;
    let mut literal = String::with_capacity(body.len());
    let mut characters = body.char_indices();
    while let Some((offset, current)) = characters.next() {
        let position = body_start + offset;
        match current {
            '\\' => literal.push(resolve_escape(&mut characters, position)?),
            control if u32::from(control) < FIRST_UNESCAPED_CODE_POINT => {
                return Err(FilterError::UnescapedControlCharacter {
                    code_point: u32::from(control),
                    position,
                });
            }
            unescaped => literal.push(unescaped),
        }
    }
    admit_held_text(&literal)?;
    Ok(literal)
}

/// Resolve the escape whose backslash sits at `escape_at` and whose remaining
/// characters are `characters`, returning the character it names.
fn resolve_escape(characters: &mut CharIndices<'_>, escape_at: usize) -> Result<char, FilterError> {
    let Some((_, named)) = characters.next() else {
        return Err(FilterError::UnterminatedString);
    };
    // RFC 8259 section 7 lists these eight and no others; anything else is a
    // malformed string rather than a literal backslash followed by a letter.
    match named {
        '"' => Ok('"'),
        '\\' => Ok('\\'),
        '/' => Ok('/'),
        'b' => Ok('\u{0008}'),
        'f' => Ok('\u{000C}'),
        'n' => Ok('\n'),
        'r' => Ok('\r'),
        't' => Ok('\t'),
        'u' => resolve_unicode_escape(characters, escape_at),
        other => Err(FilterError::InvalidStringEscape {
            escape: other,
            position: escape_at,
        }),
    }
}

/// Resolve a `\uXXXX` escape whose `\u` is consumed, joining a surrogate pair
/// when the first code unit is a high surrogate.
fn resolve_unicode_escape(characters: &mut CharIndices<'_>, escape_at: usize) -> Result<char, FilterError> {
    let unit = read_code_unit(characters, escape_at)?;
    if let Some(scalar) = char::from_u32(u32::from(unit)) {
        return Ok(scalar);
    }
    // `char::from_u32` refuses exactly the UTF-16 surrogate range, so reaching
    // here means the escape named a surrogate: a high one is only well-formed
    // when a low one completes it, and a low one alone never is.
    if (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(&unit) {
        resolve_surrogate_pair(characters, unit, escape_at)
    } else {
        Err(FilterError::InvalidUnicodeEscape {
            position: escape_at,
            error: UnicodeEscapeError::UnpairedLowSurrogate,
        })
    }
}

/// Join the high surrogate escaped at `high_escape_at` with the `\u` low
/// surrogate that must follow it in `characters`.
fn resolve_surrogate_pair(characters: &mut CharIndices<'_>, high: u16, high_escape_at: usize) -> Result<char, FilterError> {
    let unpaired = || FilterError::InvalidUnicodeEscape {
        position: high_escape_at,
        error: UnicodeEscapeError::UnpairedHighSurrogate,
    };
    if !characters.as_str().starts_with("\\u") {
        return Err(unpaired());
    }
    characters.nth(UNICODE_ESCAPE_PREFIX - 1);
    let low_escape_at = high_escape_at + UNICODE_ESCAPE_PREFIX + UNICODE_ESCAPE_DIGITS;
    let low = read_code_unit(characters, low_escape_at)?;
    match char::decode_utf16([high, low]).next() {
        Some(Ok(scalar)) => Ok(scalar),
        Some(Err(_)) | None => Err(unpaired()),
    }
}

/// Read the four hexadecimal digits that follow a consumed `\u` at `escape_at`.
fn read_code_unit(characters: &mut CharIndices<'_>, escape_at: usize) -> Result<u16, FilterError> {
    let not_four_hex_digits = || FilterError::InvalidUnicodeEscape {
        position: escape_at,
        error: UnicodeEscapeError::NotFourHexDigits,
    };
    let digits = characters
        .as_str()
        .get(..UNICODE_ESCAPE_DIGITS)
        .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(not_four_hex_digits)?;
    let unit = u16::from_str_radix(digits, 16).map_err(|_| not_four_hex_digits())?;
    characters.nth(UNICODE_ESCAPE_DIGITS - 1);
    Ok(unit)
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            expectation::{Expectation, Found},
            filter_error::FilterError,
            parser::quoted_string::{QuotedLiteral, decode_quoted_string, quoted_literal},
            unicode_escape_error::UnicodeEscapeError,
        },
        query_text::grammar_fault::GrammarFault,
    };
    use winnow::{
        Parser,
        stream::{LocatingSlice, Location, Stream},
    };

    fn read<'i>(stream: &mut LocatingSlice<&'i str>, text: &str) -> Result<QuotedLiteral<'i>, FilterError> {
        quoted_literal.parse_next(stream).map_err(|fault| {
            fault.finish(|position| FilterError::Expected {
                expected: Expectation::Value,
                found: Found::of(text.get(position..).and_then(|rest| rest.chars().next())),
                position,
            })
        })
    }

    fn scan(input: &str) -> Result<(String, usize), FilterError> {
        let mut stream = LocatingSlice::new(input);
        let literal = read(&mut stream, input)?;
        Ok((decode_quoted_string(literal.body, literal.start)?, stream.current_token_start()))
    }

    fn scanned(input: &str) -> String {
        scan(input).expect("a well-formed JSON string").0
    }

    #[test]
    fn the_eight_two_character_escapes_resolve_to_the_characters_rfc_8259_names() {
        pretty_assertions::assert_eq!(scanned(r#""a\"b""#), "a\"b");
        pretty_assertions::assert_eq!(scanned(r#""a\\b""#), "a\\b");
        pretty_assertions::assert_eq!(scanned(r#""a\/b""#), "a/b");
        pretty_assertions::assert_eq!(scanned(r#""a\bb""#), "a\u{0008}b");
        pretty_assertions::assert_eq!(scanned(r#""a\fb""#), "a\u{000C}b");
        pretty_assertions::assert_eq!(scanned(r#""a\nb""#), "a\nb");
        pretty_assertions::assert_eq!(scanned(r#""a\rb""#), "a\rb");
        pretty_assertions::assert_eq!(scanned(r#""a\tb""#), "a\tb");
    }

    /// The escape of a code unit: a reverse solidus, `u` and the digits, built
    /// apart from the test text so that no tool rewrites it into the character.
    fn unicode_escape(digits: &str) -> String {
        format!("{}u{digits}", '\\')
    }

    fn quoted(body: &str) -> String {
        format!("\"{body}\"")
    }

    #[test]
    fn a_unicode_escape_resolves_to_its_code_point_rather_than_to_its_spelling() {
        pretty_assertions::assert_eq!(scanned(&quoted(&format!("a{}", unicode_escape("00e9")))), "a\u{e9}");
        pretty_assertions::assert_eq!(scanned(&quoted(&unicode_escape("0041"))), "A");
        pretty_assertions::assert_eq!(scanned(&quoted(&unicode_escape("00E9"))), "\u{e9}", "hexadecimal digits are case-insensitive");
    }

    #[test]
    fn a_surrogate_pair_resolves_to_one_code_point_above_the_basic_multilingual_plane() {
        let pair = format!("{}{}", unicode_escape("d83d"), unicode_escape("de00"));
        pretty_assertions::assert_eq!(scanned(&quoted(&pair)), "\u{1F600}");
        pretty_assertions::assert_eq!(scanned(&quoted(&format!("a{pair}b"))), "a\u{1F600}b");
        pretty_assertions::assert_eq!(scanned(&quoted(&format!("{}{}", unicode_escape("D83D"), unicode_escape("DE00")))), "\u{1F600}");
    }

    #[test]
    fn an_escape_the_json_grammar_does_not_define_is_refused() {
        let error = scan(r#""a\qb""#).expect_err("`\\q` is not a JSON escape");
        assert!(matches!(error, FilterError::InvalidStringEscape { escape: 'q', position: 2 }), "got {error:?}");
    }

    #[test]
    fn an_escape_position_counts_bytes_from_the_start_of_the_literal() {
        let error = decode_quoted_string(r"é\q", 10).expect_err("`\\q` is not a JSON escape");
        assert!(matches!(error, FilterError::InvalidStringEscape { escape: 'q', position: 13 }), "got {error:?}");
    }

    #[test]
    fn a_unicode_escape_without_four_hexadecimal_digits_is_refused() {
        for input in [r#""\u12""#, r#""\u12g4""#, r#""\u""#, r#""\u12é""#] {
            let error = scan(input).expect_err("`\\u` requires four hexadecimal digits");
            assert!(
                matches!(
                    error,
                    FilterError::InvalidUnicodeEscape {
                        error: UnicodeEscapeError::NotFourHexDigits,
                        position: 1,
                    }
                ),
                "{input}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_lone_surrogate_is_refused_in_either_half_of_the_range() {
        let high = scan(r#""\ud83d""#).expect_err("a high surrogate needs a low one");
        assert!(
            matches!(
                high,
                FilterError::InvalidUnicodeEscape {
                    error: UnicodeEscapeError::UnpairedHighSurrogate,
                    position: 1,
                }
            ),
            "got {high:?}"
        );
        let low = scan(r#""\ude00""#).expect_err("a low surrogate never stands alone");
        assert!(
            matches!(
                low,
                FilterError::InvalidUnicodeEscape {
                    error: UnicodeEscapeError::UnpairedLowSurrogate,
                    position: 1,
                }
            ),
            "got {low:?}"
        );
        let mismatched = scan(r#""\ud83dA""#).expect_err("a high surrogate needs a low one, not a letter");
        assert!(
            matches!(
                mismatched,
                FilterError::InvalidUnicodeEscape {
                    error: UnicodeEscapeError::UnpairedHighSurrogate,
                    position: 1,
                }
            ),
            "got {mismatched:?}"
        );
    }

    #[test]
    fn a_second_escape_of_a_surrogate_pair_that_is_malformed_is_refused_at_its_own_backslash() {
        let error = scan(r#""\ud83d\u12""#).expect_err("the low half needs four hexadecimal digits");
        assert!(
            matches!(
                error,
                FilterError::InvalidUnicodeEscape {
                    error: UnicodeEscapeError::NotFourHexDigits,
                    position: 7,
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn an_unescaped_control_character_is_refused() {
        let error = scan("\"a\u{0007}b\"").expect_err("the `unescaped` production starts at U+0020");
        assert!(
            matches!(error, FilterError::UnescapedControlCharacter { code_point: 0x07, position: 2 }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_string_that_is_never_closed_is_refused() {
        assert!(matches!(scan(r#""abc"#), Err(FilterError::UnterminatedString)));
        assert!(matches!(scan(r#""abc\"#), Err(FilterError::UnterminatedString)));
    }

    #[test]
    fn an_unescaped_character_at_or_above_the_boundary_is_kept_verbatim() {
        pretty_assertions::assert_eq!(scanned(r#"" ""#), " ", "U+0020 opens the unescaped range");
        pretty_assertions::assert_eq!(scanned(r#""urn:ngsi-ld:Vehicle:A4567%2Fb""#), "urn:ngsi-ld:Vehicle:A4567%2Fb");
        pretty_assertions::assert_eq!(scanned(r#""é日本""#), "é日本");
    }

    /// Clause 4.9 `QueryTerm =/ Attribute patternOp RegExp` delegates `RegExp`
    /// to IEEE 1003.2, not to `quotedStr`, so the scanner must hand a pattern's
    /// backslashes on untouched. Every one of these is a well-formed POSIX
    /// extended regular expression and none is a well-formed JSON string.
    #[test]
    fn a_pattern_body_survives_the_scan_with_its_backslashes_intact() {
        for pattern in [r"^example\.com$", r"^\(a\)$", r"a\+", r"[[:alpha:]]\{2,3\}"] {
            let quoted = format!("\"{pattern}\"");
            let mut stream = LocatingSlice::new(quoted.as_str());
            let literal = read(&mut stream, &quoted).expect("a pattern is lexically a quoted literal");
            pretty_assertions::assert_eq!(literal.body, pattern);
            assert!(
                decode_quoted_string(literal.body, literal.start).is_err(),
                "{pattern} is not a well-formed JSON string"
            );
        }
    }

    /// An escaped quotation mark must not end the literal, even though the
    /// escape itself is left for the operand parser to resolve.
    #[test]
    fn an_escaped_quotation_mark_does_not_close_the_literal() {
        let text = r#""say \"hi\"" rest"#;
        let mut stream = LocatingSlice::new(text);
        let literal = read(&mut stream, text).expect("a well-formed quoted literal");
        pretty_assertions::assert_eq!(literal.body, r#"say \"hi\""#);
        pretty_assertions::assert_eq!(decode_quoted_string(literal.body, literal.start).expect("eight escapes"), r#"say "hi""#);
        pretty_assertions::assert_eq!(stream.peek_finish(), " rest");
    }

    #[test]
    fn the_stream_follows_the_closing_quotation_mark() {
        let text = r#""ab"c"#;
        let mut stream = LocatingSlice::new(text);
        let literal = read(&mut stream, text).expect("a well-formed quoted literal");
        pretty_assertions::assert_eq!(literal.body, "ab");
        pretty_assertions::assert_eq!(stream.peek_finish(), "c");
    }

    #[test]
    fn a_literal_records_where_its_opening_quotation_mark_stands() {
        let text = "a==\"x\"";
        let mut stream = LocatingSlice::new(text);
        let _operator = stream.next_slice(3);
        let literal = read(&mut stream, text).expect("a well-formed quoted literal");
        pretty_assertions::assert_eq!(literal, QuotedLiteral { body: "x", start: 3 });
    }

    #[test]
    fn a_text_that_does_not_open_with_a_quotation_mark_backtracks_without_consuming() {
        let mut stream = LocatingSlice::new("x\"");
        let result = quoted_literal.parse_next(&mut stream);
        assert!(matches!(result, Err(GrammarFault::Backtrack { position: 0 })), "got {result:?}");
        pretty_assertions::assert_eq!(stream.current_token_start(), 0);
    }

    /// Clause 4.9 reads `\u0000` as U+0000, which no stored value can hold,
    /// so the literal is refused rather than compared.
    #[test]
    fn an_escape_spelling_u0000_is_refused() {
        assert!(matches!(decode_quoted_string(r"a\u0000b", 0), Err(FilterError::UnholdableCharacter(_))));
        pretty_assertions::assert_eq!(decode_quoted_string(r"\\u0000", 0).expect("an escaped reverse solidus"), r"\u0000");
    }
}
