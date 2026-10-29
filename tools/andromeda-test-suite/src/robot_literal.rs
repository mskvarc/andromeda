//! How the ETSI keywords read a received body: `json.loads('''<body>''')`.
//!
//! The notification and context source keywords paste the body into a Python `'''...'''` literal before parsing it,
//! so a backslash is read as an escape, and a `'''` or a final `'` ends the literal. A port refuses all three and
//! then requires the body to be JSON, which is stricter only where an escape leaves the keyword reading, and
//! accepting, a body other than the one sent.

use serde_json::Value;
use std::str;

/// Why a body is not one the keyword reads as sent.
#[derive(Debug, thiserror::Error)]
pub enum LiteralError {
    /// The body is not UTF-8.
    #[error("the body is not UTF-8")]
    Utf8(#[from] str::Utf8Error),
    /// The body holds a backslash, which the literal reads as an escape.
    #[error("the body holds a backslash at byte {0}")]
    Backslash(usize),
    /// The body holds `'''`, which ends the literal.
    #[error("the body holds ''' at byte {0}")]
    TripleQuote(usize),
    /// The body ends with `'`, which ends the literal.
    #[error("the body ends with '")]
    FinalQuote,
    /// The body is not JSON.
    #[error("the body is not JSON")]
    Json(#[from] serde_json::Error),
}

/// The JSON document the keyword reads from `body`.
///
/// # Errors
///
/// Returns [`LiteralError`] when the body breaks the literal or is not JSON.
pub fn read(body: &[u8]) -> Result<Value, LiteralError> {
    let text = str::from_utf8(body)?;
    if let Some(at) = text.find('\\') {
        return Err(LiteralError::Backslash(at));
    }
    if let Some(at) = text.find("'''") {
        return Err(LiteralError::TripleQuote(at));
    }
    if text.ends_with('\'') {
        return Err(LiteralError::FinalQuote);
    }
    Ok(serde_json::from_str(text)?)
}

#[cfg(test)]
mod tests {
    use crate::robot_literal::read;
    use googletest::prelude::*;
    use serde_json::json;

    #[gtest]
    fn a_plain_json_body_is_read() -> Result<()> {
        verify_that!(read(br#"{"id": "urn:a"}"#)?, eq(&json!({"id": "urn:a"})))
    }

    #[gtest]
    fn a_body_breaking_the_literal_is_refused() {
        expect_that!(read(br#"{"a": "\n"}"#), err(displays_as(starts_with("the body holds a backslash"))));
        expect_that!(read(br#"{"a": "'''"}"#), err(displays_as(starts_with("the body holds '''"))));
        expect_that!(read(br#"{"a": 1}'"#), err(displays_as(starts_with("the body ends with '"))));
        expect_that!(read(b"not json"), err(displays_as(eq("the body is not JSON"))));
    }
}
