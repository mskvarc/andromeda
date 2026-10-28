//! The one character no text a client hands the broker may hold.
//!
//! JSON admits U+0000 in a string (IETF RFC 8259 section 7), a URI query
//! carries it as `%00`, and ETSI GS CIM 009 v1.9.1 sets no rule against it,
//! but PostgreSQL holds the character neither in `text` nor in `jsonb`, so a
//! request carrying it could only fail at the statement that reaches the store.
//! It is refused where client text is decoded instead, as input the operation
//! cannot take (clause 5.5.4, `BadRequestData`): every decoder of client text
//! holds what it decodes to [`admit_held_text`], and nothing downstream of
//! those decoders checks again.

use thiserror::Error;

/// A text holds U+0000, which the broker cannot hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the text holds the character U+0000, which this broker cannot hold")]
pub struct UnholdableCharacter;

/// Admit `text` as one the broker can hold.
///
/// # Errors
///
/// Returns [`UnholdableCharacter`] when `text` holds U+0000.
pub fn admit_held_text(text: &str) -> Result<(), UnholdableCharacter> {
    if text.contains('\0') { Err(UnholdableCharacter) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use crate::held_text::{UnholdableCharacter, admit_held_text};

    #[test]
    fn a_text_holding_u0000_anywhere_is_refused() {
        for text in ["\0", "a\0", "\0b", "a\0b"] {
            pretty_assertions::assert_eq!(admit_held_text(text), Err(UnholdableCharacter), "{text:?}");
        }
    }

    #[test]
    fn every_other_control_character_and_the_escape_spelling_are_held() {
        for text in ["", "plain", "\u{1}\u{1F}\u{7F}", "\\u0000", "%00"] {
            pretty_assertions::assert_eq!(admit_held_text(text), Ok(()), "{text:?}");
        }
    }
}
