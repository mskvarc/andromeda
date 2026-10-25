//! Free-form text a resource member requires to be a non-empty string.

use derive_more::{AsRef, Display, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// A human-readable string that clause 5.2.9 restricts to "Non-empty string".
///
/// `registrationName` and `description` carry no structure beyond that
/// restriction, so the type guards exactly it: the text is free-form, but an
/// empty value is a request the broker must reject rather than store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Into, Serialize, Deserialize)]
#[display("{_0}")]
#[as_ref(str)]
#[into(String)]
#[serde(try_from = "String", into = "String")]
pub struct NonEmptyText(String);

/// Why a string cannot be a [`NonEmptyText`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NonEmptyTextError {
    /// The string was empty.
    #[error("text must be a non-empty string")]
    Empty,
}

impl NonEmptyText {
    /// Validate that `text` is non-empty.
    ///
    /// # Errors
    ///
    /// Returns [`NonEmptyTextError::Empty`] for an empty string.
    pub fn new(text: impl Into<String>) -> Result<NonEmptyText, NonEmptyTextError> {
        let text = text.into();
        if text.is_empty() {
            return Err(NonEmptyTextError::Empty);
        }
        Ok(NonEmptyText(text))
    }

    /// The text as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NonEmptyText {
    type Error = NonEmptyTextError;

    fn try_from(value: String) -> Result<NonEmptyText, NonEmptyTextError> {
        NonEmptyText::new(value)
    }
}

impl FromStr for NonEmptyText {
    type Err = NonEmptyTextError;

    fn from_str(value: &str) -> Result<NonEmptyText, NonEmptyTextError> {
        NonEmptyText::new(value)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::non_empty_text::{NonEmptyText, NonEmptyTextError};

    #[test]
    fn a_non_empty_string_is_accepted_and_round_trips() {
        let text = NonEmptyText::new("vehicle telemetry").expect("non-empty text");
        pretty_assertions::assert_eq!(text.as_str(), "vehicle telemetry");
        pretty_assertions::assert_eq!(text.to_string(), "vehicle telemetry");
    }

    #[test]
    fn an_empty_string_is_rejected() {
        pretty_assertions::assert_eq!(NonEmptyText::new(""), Err(NonEmptyTextError::Empty));
        assert!("".parse::<NonEmptyText>().is_err());
    }

    #[test]
    fn serde_rejects_an_empty_string_and_keeps_a_non_empty_one_verbatim() {
        let parsed: NonEmptyText = serde_json::from_str("\"a name\"").expect("non-empty parses");
        pretty_assertions::assert_eq!(serde_json::to_string(&parsed).expect("serialises"), "\"a name\"");
        assert!(serde_json::from_str::<NonEmptyText>("\"\"").is_err());
    }
}
