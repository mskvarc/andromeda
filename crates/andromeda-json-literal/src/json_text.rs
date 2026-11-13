//! Writing JSON text by hand from values already admitted.
//!
//! A document assembled from the pieces of admitted literals (a `@context`
//! array built from the contexts a request named, a definition written without
//! one of its members) needs no tree: each piece is already canonical JSON
//! text, and only the strings the broker supplies itself have to be quoted.

use jstrict::print::string_literal;
use std::fmt;

/// `text` as a JSON string, quoted and escaped as IETF RFC 8259 section 7
/// requires: the quotation mark, the reverse solidus and every control
/// character are escaped, the short forms used where the grammar has one and
/// a lowercase `\u00xx` otherwise, as IETF RFC 8785 section 3.2.2.2 spells
/// them.
#[must_use]
pub fn quoted(text: &str) -> String {
    fmt::from_fn(|formatter| string_literal(text, formatter)).to_string()
}

#[cfg(test)]
mod tests {
    use crate::json_text::quoted;

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
    fn each_control_character_with_a_short_form_is_written_in_it() {
        pretty_assertions::assert_eq!(quoted("\u{08}\u{0C}\r\n\t"), r#""\b\f\r\n\t""#);
    }

    #[test]
    fn every_control_character_is_escaped_and_reads_back() {
        let text: String = (0x00_u8..0x20).map(char::from).collect();
        let written = quoted(&text);
        assert!(written.bytes().all(|byte| byte >= 0x20), "{written}");
        let read: String = serde_json::from_str(&written).expect("the quoted text is a JSON string");
        pretty_assertions::assert_eq!(read, text);
    }

    /// A text of 64 bytes or more takes another encoder than a short one, and
    /// is spelled the same.
    #[test]
    fn a_long_text_is_escaped_as_its_short_pieces_are() {
        let piece = "a\u{01}\u{08}\"\\\u{1F}é\n";
        let short = quoted(piece);
        let inner = short.trim_start_matches('"').trim_end_matches('"');
        let long = piece.repeat(16);
        assert!(long.len() >= 64);
        pretty_assertions::assert_eq!(quoted(&long), format!("\"{}\"", inner.repeat(16)));
    }

    #[test]
    fn a_quoted_text_reads_back_as_the_text() {
        let text = "tab\there \"quoted\" \u{07} é";
        let read: String = serde_json::from_str(&quoted(text)).expect("the quoted text is a JSON string");
        pretty_assertions::assert_eq!(read, text);
    }
}
