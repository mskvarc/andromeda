//! The message of a failed decoding, without the place the reader failed at.
//!
//! A JSON reader appends the line and column it stopped at to every failure,
//! and a failure decoding one member counts them from that member's own text.
//! Neither locates anything for a client: a refusal names the member it
//! concerns instead, so it carries the message alone.

use derive_more::Display;

/// The message `error` carries, without the line and column it was read at.
#[derive(Debug, Clone, Copy, Display)]
#[display("{}", without_position(_0))]
pub struct DecodeMessage<'e>(pub &'e serde_json::Error);

/// The text of `error` without the ` at line L column C` its reader appended.
///
/// The reader appends the position to the message only when it counted one,
/// so a failure raised outside a reader (line 0) is its message as it stands.
fn without_position(error: &serde_json::Error) -> String {
    let text = error.to_string();
    let position = format!(" at line {} column {}", error.line(), error.column());
    match text.strip_suffix(&position) {
        Some(message) => message.to_owned(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use crate::decode_message::DecodeMessage;
    use serde::de::Error as DecodeError;

    #[test]
    fn a_reader_failure_is_its_message_without_the_line_and_column() {
        let error = serde_json::from_str::<u16>("\n  \"x\"").expect_err("a string is no number");
        pretty_assertions::assert_eq!(error.line(), 2, "{error}");
        pretty_assertions::assert_eq!(DecodeMessage(&error).to_string(), r#"invalid type: string "x", expected u16"#);
    }

    #[test]
    fn a_failure_raised_outside_a_reader_is_its_message() {
        let error = serde_json::Error::custom("the member 'id' is missing");
        pretty_assertions::assert_eq!(DecodeMessage(&error).to_string(), "the member 'id' is missing");
    }

    /// A message that itself ends in words like a position keeps them: only
    /// the position the reader counted is taken off.
    #[test]
    fn only_the_counted_position_is_taken_off() {
        let error = serde_json::from_str::<u16>(r#""at line 9 column 9""#).expect_err("a string is no number");
        pretty_assertions::assert_eq!(DecodeMessage(&error).to_string(), r#"invalid type: string "at line 9 column 9", expected u16"#);
    }
}
