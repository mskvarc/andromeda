//! Validated callback request paths.

use thiserror::Error;

/// Absolute HTTP path used to match a callback request.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct CallbackPath {
    value: String,
}

impl CallbackPath {
    /// Validate a slash-prefixed callback path.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackPathError`] when `value` does not begin with `/`.
    pub fn parse(value: impl Into<String>) -> Result<CallbackPath, CallbackPathError> {
        let value = value.into();
        if value.starts_with('/') {
            Ok(CallbackPath { value })
        } else {
            Err(CallbackPathError)
        }
    }

    /// Borrow the encoded path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

/// Invalid callback path syntax.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("callback path must begin with '/'")]
pub struct CallbackPathError;
