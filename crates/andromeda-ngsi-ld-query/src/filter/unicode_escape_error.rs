//! The reason a `\u` escape of a `quotedStr` literal is refused.

use derive_more::Display;

/// Why a `\u` escape did not name a Unicode scalar value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum UnicodeEscapeError {
    /// Fewer than four hexadecimal digits followed the `\u` introducer.
    #[display("`\\u` was not followed by four hexadecimal digits")]
    NotFourHexDigits,
    /// A high surrogate was not completed by a `\u` low surrogate.
    #[display("a high surrogate was not completed by a low surrogate")]
    UnpairedHighSurrogate,
    /// A low surrogate appeared with no high surrogate before it.
    #[display("a low surrogate appeared without a preceding high surrogate")]
    UnpairedLowSurrogate,
}
