//! Writing JSON text by hand from values already admitted.
//!
//! A document assembled from the pieces of admitted literals -- a `@context`
//! array built from the contexts a request named, a definition written without
//! one of its members -- needs no tree: each piece is already canonical JSON
//! text, and only the strings the broker supplies itself have to be quoted.

/// `text` as a JSON string, quoted and escaped as IETF RFC 8259 section 7
/// requires: the quotation mark, the reverse solidus and every control
/// character are escaped, the short forms used where the grammar has one.
#[must_use]
pub fn quoted(text: &str) -> String {
    /// The hexadecimal digits a `\u` escape spells a control character with.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut written = String::with_capacity(text.len() + 2);
    written.push('"');
    for character in text.chars() {
        match character {
            '"' => written.push_str("\\\""),
            '\\' => written.push_str("\\\\"),
            '\n' => written.push_str("\\n"),
            '\r' => written.push_str("\\r"),
            '\t' => written.push_str("\\t"),
            '\u{08}' => written.push_str("\\b"),
            '\u{0C}' => written.push_str("\\f"),
            control @ '\u{00}'..='\u{1F}' => {
                let code = u32::from(control);
                written.push_str("\\u00");
                written.extend(
                    [code >> 4, code & 0xF]
                        .into_iter()
                        .filter_map(|nibble| usize::try_from(nibble).ok().and_then(|index| HEX.get(index)).map(|digit| char::from(*digit))),
                );
            }
            other => written.push(other),
        }
    }
    written.push('"');
    written
}

/// Whether the well-formed JSON text `text` holds U+0000 in one of its
/// strings or member names.
///
/// IETF RFC 8259 section 7 admits the character in a string only as the
/// escape `\u0000`, and every reverse solidus of a well-formed text opens an
/// escape, so reading the escapes in turn finds it without decoding a string.
#[must_use]
pub fn holds_nul_character(text: &str) -> bool {
    let mut rest = text;
    while let Some((_, escape)) = rest.split_once('\\') {
        if escape.starts_with("u0000") {
            return true;
        }
        rest = escape.get(1..).unwrap_or_default();
    }
    false
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::json_text::{holds_nul_character, quoted};

    #[test]
    fn plain_text_is_only_quoted() {
        pretty_assertions::assert_eq!(quoted("https://example.com/context.jsonld"), r#""https://example.com/context.jsonld""#);
    }

    /// IETF RFC 8259 section 7: the quotation mark, the reverse solidus and
    /// the control characters are escaped, short forms where they exist.
    #[test]
    fn the_characters_the_grammar_reserves_are_escaped() {
        pretty_assertions::assert_eq!(quoted("a\"b\\c\nd\te\u{01}f\u{1F}"), r#""a\"b\\c\nd\te\u0001f\u001f""#);
    }

    #[test]
    fn a_quoted_text_reads_back_as_the_text() {
        let text = "tab\there \"quoted\" \u{07} é";
        let read: String = serde_json::from_str(&quoted(text)).expect("the quoted text is a JSON string");
        pretty_assertions::assert_eq!(read, text);
    }

    #[test]
    fn a_nul_escape_in_a_string_or_a_member_name_is_found() {
        assert!(holds_nul_character(r#"{"a":"x\u0000y"}"#));
        assert!(holds_nul_character(r#"{"a\u0000":1}"#));
        assert!(holds_nul_character(&quoted("nul\u{0}here")));
    }

    #[test]
    fn an_escaped_reverse_solidus_before_u0000_is_no_nul() {
        assert!(!holds_nul_character(r#"["\\u0000", "\u0001", "\"u0000"]"#));
        assert!(!holds_nul_character(r#"{"a":[1,true,null]}"#));
    }
}
