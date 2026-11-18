//! The JSON string production the clause 4.9 `quotedStr` terminal delegates to.
//!
//! Clause 4.9 defines `quotedStr = String` and mandates that both `String` and
//! `char` follow the ABNF of IETF RFC 8259 section 7. That grammar admits
//! exactly eight two-character escapes plus `\uXXXX`, forbids an unescaped
//! control character, and spells a code point above the BMP as a UTF-16
//! surrogate pair. Resolving an arbitrary backslash pair to its second
//! character instead would make `q=name=="aé"` match the literal
//! `au00e9`, and would silently accept escapes the grammar does not define.

use crate::filter::{filter_error::FilterError, unicode_escape_error::UnicodeEscapeError};
use andromeda_ngsi_ld_number::held_text::admit_held_text;

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

/// Find the end of a double-quoted literal and return its body verbatim.
///
/// `start` indexes the opening quotation mark; the returned index is the one
/// that follows the closing quotation mark. A backslash defers to the character
/// after it, so an escaped quotation mark does not end the literal, but no
/// escape is *resolved* here, because which resolution applies depends on the
/// production the literal stands in. Clause 4.9 puts a `quotedStr` and a
/// `RegExp` in the same lexical shape and gives them different grammars:
/// `quotedStr = String` follows IETF RFC 8259 section 7, while `RegExp` is
/// delegated to IEEE 1003.2, whose own metacharacters include the backslash.
/// Resolving RFC 8259 escapes here would refuse `^example\.com$`.
///
/// The body is otherwise taken verbatim: percent-encoding is the HTTP binding's
/// concern (clause 4.9) and is decoded once before a filter reaches this
/// scanner, so a second decode here would rewrite a URN that legitimately
/// carries `%2F`.
///
/// # Errors
///
/// Returns [`FilterError::UnterminatedString`] when the literal is left open.
pub(super) fn scan_quoted_literal(chars: &[char], start: usize) -> Result<(String, usize), FilterError> {
    let mut body = String::new();
    let mut cursor = start + 1;
    loop {
        let Some(&current) = chars.get(cursor) else {
            return Err(FilterError::UnterminatedString);
        };
        match current {
            '"' => return Ok((body, cursor + 1)),
            '\\' => {
                let Some(&deferred) = chars.get(cursor + 1) else {
                    return Err(FilterError::UnterminatedString);
                };
                body.push('\\');
                body.push(deferred);
                cursor += 2;
            }
            other => {
                body.push(other);
                cursor += 1;
            }
        }
    }
}

/// Read the body of a quoted literal as the clause 4.9 `quotedStr` production,
/// which IETF RFC 8259 section 7 defines.
///
/// That grammar admits exactly eight two-character escapes plus `\uXXXX`,
/// forbids an unescaped control character, and spells a code point above the
/// BMP as a UTF-16 surrogate pair. Resolving an arbitrary backslash pair to its
/// second character instead would make `q=name=="aé"` match the literal
/// `au00e9`.
///
/// Positions in the returned errors are offsets into `body`, which is the text
/// between the quotation marks rather than the whole `q` filter.
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
pub(super) fn decode_quoted_string(body: &str) -> Result<String, FilterError> {
    let chars: Vec<char> = body.chars().collect();
    let mut literal = String::new();
    let mut cursor = 0;
    while let Some(&current) = chars.get(cursor) {
        match current {
            '\\' => {
                let (resolved, next) = resolve_escape(&chars, cursor)?;
                literal.push(resolved);
                cursor = next;
            }
            control if u32::from(control) < FIRST_UNESCAPED_CODE_POINT => {
                return Err(FilterError::UnescapedControlCharacter {
                    code_point: u32::from(control),
                    position: cursor,
                });
            }
            unescaped => {
                literal.push(unescaped);
                cursor += 1;
            }
        }
    }
    admit_held_text(&literal)?;
    Ok(literal)
}

/// Resolve the escape whose backslash sits at `escape_at`, returning the
/// character it names and the index that follows it.
fn resolve_escape(chars: &[char], escape_at: usize) -> Result<(char, usize), FilterError> {
    let Some(&named) = chars.get(escape_at + 1) else {
        return Err(FilterError::UnterminatedString);
    };
    // RFC 8259 section 7 lists these eight and no others; anything else is a
    // malformed string rather than a literal backslash followed by a letter.
    let simple = match named {
        '"' => Some('"'),
        '\\' => Some('\\'),
        '/' => Some('/'),
        'b' => Some('\u{0008}'),
        'f' => Some('\u{000C}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'u' => None,
        other => {
            return Err(FilterError::InvalidStringEscape {
                escape: other,
                position: escape_at,
            });
        }
    };
    match simple {
        Some(resolved) => Ok((resolved, escape_at + 2)),
        None => resolve_unicode_escape(chars, escape_at),
    }
}

/// Resolve a `\uXXXX` escape, joining a surrogate pair when the first code
/// unit is a high surrogate.
fn resolve_unicode_escape(chars: &[char], escape_at: usize) -> Result<(char, usize), FilterError> {
    let unit = read_code_unit(chars, escape_at)?;
    let after_unit = escape_at + UNICODE_ESCAPE_PREFIX + UNICODE_ESCAPE_DIGITS;
    if let Some(scalar) = char::from_u32(u32::from(unit)) {
        return Ok((scalar, after_unit));
    }
    // `char::from_u32` refuses exactly the UTF-16 surrogate range, so reaching
    // here means the escape named a surrogate: a high one is only well-formed
    // when a low one completes it, and a low one alone never is.
    if (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(&unit) {
        resolve_surrogate_pair(chars, unit, escape_at)
    } else {
        Err(FilterError::InvalidUnicodeEscape {
            position: escape_at,
            error: UnicodeEscapeError::UnpairedLowSurrogate,
        })
    }
}

/// Join the high surrogate at `high_escape_at` with the `\u` low surrogate
/// that must follow it.
fn resolve_surrogate_pair(chars: &[char], high: u16, high_escape_at: usize) -> Result<(char, usize), FilterError> {
    let unpaired = || FilterError::InvalidUnicodeEscape {
        position: high_escape_at,
        error: UnicodeEscapeError::UnpairedHighSurrogate,
    };
    let low_escape_at = high_escape_at + UNICODE_ESCAPE_PREFIX + UNICODE_ESCAPE_DIGITS;
    if chars.get(low_escape_at) != Some(&'\\') || chars.get(low_escape_at + 1) != Some(&'u') {
        return Err(unpaired());
    }
    let low = read_code_unit(chars, low_escape_at)?;
    let after_low = low_escape_at + UNICODE_ESCAPE_PREFIX + UNICODE_ESCAPE_DIGITS;
    match char::decode_utf16([high, low]).next() {
        Some(Ok(scalar)) => Ok((scalar, after_low)),
        Some(Err(_)) | None => Err(unpaired()),
    }
}

/// Read the four hexadecimal digits that follow the `\u` at `escape_at`.
fn read_code_unit(chars: &[char], escape_at: usize) -> Result<u16, FilterError> {
    let not_four_hex_digits = || FilterError::InvalidUnicodeEscape {
        position: escape_at,
        error: UnicodeEscapeError::NotFourHexDigits,
    };
    let first_digit = escape_at + UNICODE_ESCAPE_PREFIX;
    let digits = chars.get(first_digit..first_digit + UNICODE_ESCAPE_DIGITS).ok_or_else(not_four_hex_digits)?;
    // Four hexadecimal digits cannot exceed `0xFFFF`, so the narrowing holds
    // for every input the loop above accepts.
    let value = digits.iter().try_fold(0_u32, |accumulated, digit| {
        digit.to_digit(16).map(|nibble| accumulated << 4 | nibble).ok_or_else(not_four_hex_digits)
    })?;
    u16::try_from(value).map_err(|_| not_four_hex_digits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::filter_error::FilterError;

    fn scan(input: &str) -> Result<(String, usize), FilterError> {
        let chars: Vec<char> = input.chars().collect();
        let (body, next) = scan_quoted_literal(&chars, 0)?;
        Ok((decode_quoted_string(&body)?, next))
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

    #[test]
    fn a_unicode_escape_resolves_to_its_code_point_rather_than_to_its_spelling() {
        pretty_assertions::assert_eq!(scanned(r#""aé""#), "aé");
        pretty_assertions::assert_eq!(scanned(r#""AB""#), "AB");
        pretty_assertions::assert_eq!(scanned(r#""é""#), "é", "hexadecimal digits are case-insensitive");
    }

    #[test]
    fn a_surrogate_pair_resolves_to_one_code_point_above_the_basic_multilingual_plane() {
        pretty_assertions::assert_eq!(scanned(r#""😀""#), "\u{1F600}");
        pretty_assertions::assert_eq!(scanned(r#""a😀b""#), "a\u{1F600}b");
    }

    #[test]
    fn an_escape_the_json_grammar_does_not_define_is_refused() {
        let error = scan(r#""a\qb""#).expect_err("`\\q` is not a JSON escape");
        assert!(matches!(error, FilterError::InvalidStringEscape { escape: 'q', position: 1 }), "got {error:?}");
    }

    #[test]
    fn a_unicode_escape_without_four_hexadecimal_digits_is_refused() {
        for input in [r#""\u12""#, r#""\u12g4""#, r#""\u""#] {
            let error = scan(input).expect_err("`\\u` requires four hexadecimal digits");
            assert!(
                matches!(
                    error,
                    FilterError::InvalidUnicodeEscape {
                        error: UnicodeEscapeError::NotFourHexDigits,
                        ..
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
                    ..
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
                    ..
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
                    ..
                }
            ),
            "got {mismatched:?}"
        );
    }

    #[test]
    fn an_unescaped_control_character_is_refused() {
        let error = scan("\"a\u{0007}b\"").expect_err("the `unescaped` production starts at U+0020");
        assert!(
            matches!(error, FilterError::UnescapedControlCharacter { code_point: 0x07, position: 1 }),
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
            let quoted: Vec<char> = format!("\"{pattern}\"").chars().collect();
            let (body, _) = scan_quoted_literal(&quoted, 0).expect("a pattern is lexically a quoted literal");
            pretty_assertions::assert_eq!(body, pattern);
            assert!(decode_quoted_string(&body).is_err(), "{pattern} is not a well-formed JSON string");
        }
    }

    /// An escaped quotation mark must not end the literal, even though the
    /// escape itself is left for the operand parser to resolve.
    #[test]
    fn an_escaped_quotation_mark_does_not_close_the_literal() {
        let chars: Vec<char> = r#""say \"hi\"" rest"#.chars().collect();
        let (body, next) = scan_quoted_literal(&chars, 0).expect("a well-formed quoted literal");
        pretty_assertions::assert_eq!(body, r#"say \"hi\""#);
        pretty_assertions::assert_eq!(decode_quoted_string(&body).expect("eight escapes"), r#"say "hi""#);
        pretty_assertions::assert_eq!(chars[next], ' ');
    }

    #[test]
    fn the_returned_index_follows_the_closing_quotation_mark() {
        let chars: Vec<char> = r#""ab"c"#.chars().collect();
        let (literal, next) = scan_quoted_literal(&chars, 0).expect("a well-formed quoted literal");
        pretty_assertions::assert_eq!(literal, "ab");
        pretty_assertions::assert_eq!(chars[next], 'c');
    }

    /// Clause 4.9 reads `\u0000` as U+0000, which no stored value can hold,
    /// so the literal is refused rather than compared.
    #[test]
    fn an_escape_spelling_u0000_is_refused() {
        assert!(matches!(decode_quoted_string(r"a\u0000b"), Err(FilterError::UnholdableCharacter(_))));
        pretty_assertions::assert_eq!(decode_quoted_string(r"\\u0000").expect("an escaped reverse solidus"), r"\u0000");
    }
}
