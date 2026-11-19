//! The `detail` of a problem: one line of prose.
//!
//! IETF RFC 9457 section 3.1.4 makes `detail` a human-readable explanation of
//! this occurrence of the problem. A line break splits that explanation across
//! a client's log or terminal, and no explanation needs one, so every line
//! break is folded into one space where a detail is made, whatever wrote the
//! text: a refusal of the broker, a library's message or a peer's answer.

use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};

/// The text of a problem's `detail`, holding no line break.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Deref, Display)]
#[serde(from = "String")]
pub struct DetailText(String);

// Normalising conversion: each line break and the whitespace around it become
// one space, which no derive writes.
impl From<String> for DetailText {
    fn from(text: String) -> DetailText {
        if text.contains(is_line_break) {
            DetailText(folded(&text))
        } else {
            DetailText(text)
        }
    }
}

// Normalising conversion, as for an owned text.
impl From<&str> for DetailText {
    fn from(text: &str) -> DetailText {
        DetailText::from(text.to_owned())
    }
}

/// Whether `character` ends a line: the Unicode line terminators, which a
/// terminal, a log viewer or a text editor each break a line at.
const fn is_line_break(character: char) -> bool {
    matches!(character, '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `text` with every line break, and the whitespace either side of it, folded
/// into one space, and none left at either end.
fn folded(text: &str) -> String {
    text.split(is_line_break)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use crate::detail_text::DetailText;

    #[test]
    fn a_text_of_one_line_is_kept_as_written() {
        for text in ["", "the member 'id' is missing", "  spaced  inside  "] {
            pretty_assertions::assert_eq!(DetailText::from(text).as_str(), text);
        }
    }

    #[test]
    fn every_line_break_is_folded_into_one_space() {
        for (text, folded) in [
            (
                "regex parse error:\n    (\n    ^\nerror: unclosed group",
                "regex parse error: ( ^ error: unclosed group",
            ),
            ("a\r\nb", "a b"),
            ("a\u{2028}b\u{2029}c\u{85}d\u{0B}e\u{0C}f", "a b c d e f"),
            ("\nleading and trailing\n\n", "leading and trailing"),
        ] {
            let detail = DetailText::from(text);
            pretty_assertions::assert_eq!(detail.as_str(), folded, "{text:?}");
        }
    }

    #[test]
    fn a_detail_read_from_a_document_is_folded_too() {
        let detail = serde_json::from_str::<DetailText>(r#""a\nb""#).expect("a string");
        pretty_assertions::assert_eq!(detail.as_str(), "a b");
    }
}
