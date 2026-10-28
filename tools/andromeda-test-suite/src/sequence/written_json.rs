//! JSON read with every number kept as written, so two documents compare equal only when each number has the same
//! spelling: `0.130000000000000005` is not `0.13`, and `4` is not `4.0`.
//!
//! Each number becomes a string holding a marker and the number's text; member order is not significant.

use serde_json::Value as Json;

/// The first character of a string that stands for a number.
const MARKER: char = '\u{0}';

/// The document `text` with every number replaced by a marked string of its text.
///
/// # Errors
///
/// Returns the parser's message when `text` is not JSON.
pub fn read(text: &str) -> Result<Json, String> {
    let mut marked = String::with_capacity(text.len() + 16);
    let mut characters = text.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some((index, character)) = characters.next() {
        if in_string {
            marked.push(character);
            match (escaped, character) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                (false, _) => {}
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                marked.push(character);
            }
            '-' | '0'..='9' => {
                let mut end = index + character.len_utf8();
                while let Some((next, following)) = characters.peek().copied() {
                    if following.is_ascii_digit() || matches!(following, '.' | 'e' | 'E' | '+' | '-') {
                        end = next + following.len_utf8();
                        characters.next();
                    } else {
                        break;
                    }
                }
                marked.push_str("\"\\u0000");
                marked.push_str(&text[index..end]);
                marked.push('"');
            }
            other => marked.push(other),
        }
    }
    serde_json::from_str(&marked).map_err(|error| error.to_string())
}

/// The JSON text of a document [`read`] gave, each number written as it was read.
#[must_use]
pub fn write(document: &Json) -> String {
    match document {
        Json::String(text) => match text.strip_prefix(MARKER) {
            Some(number) => number.to_owned(),
            None => Json::String(text.clone()).to_string(),
        },
        Json::Array(items) => format!("[{}]", items.iter().map(write).collect::<Vec<_>>().join(",")),
        Json::Object(members) => format!(
            "{{{}}}",
            members
                .iter()
                .map(|(name, value)| format!("{}:{}", Json::String(name.clone()), write(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        other @ (Json::Null | Json::Bool(_) | Json::Number(_)) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::written_json::{read, write};
    use googletest::prelude::*;

    #[gtest]
    fn numbers_compare_by_spelling() -> Result<()> {
        let read = |text: &str| read(text).ok().or_fail();
        expect_that!(read(r#"{"v": 0.130000000000000005}"#)? == read(r#"{"v": 0.13}"#)?, eq(false));
        expect_that!(read(r#"{"v": 4}"#)? == read(r#"{"v": 4.0}"#)?, eq(false));
        verify_that!(read(r#"{"a": 1, "b": [2e5]}"#)? == read(r#"{"b": [2e5], "a": 1}"#)?, eq(true))
    }

    #[gtest]
    fn a_document_is_written_back_with_its_numbers() -> Result<()> {
        let read = |text: &str| read(text).ok().or_fail();
        verify_that!(
            write(&read(r#"{"v": -0.130000000000000005, "s": "1 \" 2"}"#)?),
            eq(r#"{"v":-0.130000000000000005,"s":"1 \" 2"}"#)
        )
    }
}
