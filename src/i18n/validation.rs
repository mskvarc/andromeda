//! Locale validation for i18n endpoints.

use crate::core::error::AndromedaError;

/// Validate a BCP 47 locale string.
///
/// # Errors
///
/// Returns [`AndromedaError`] with a malformed-request kind if the locale
/// string is not a valid BCP 47 language tag.
pub fn validate_locale(locale: &str) -> Result<(), AndromedaError> {
    langtag::LangTagBuf::new(locale.to_string())
        .map(|_| ())
        .map_err(|error| AndromedaError::malformed_request(format!("Invalid locale value: {}", error)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn validate_locale_valid_bcp47() {
        assert!(validate_locale("en").is_ok());
        assert!(validate_locale("en-US").is_ok());
        assert!(validate_locale("de-DE").is_ok());
        assert!(validate_locale("zh-Hans-CN").is_ok());
    }

    #[test]
    fn validate_locale_invalid() {
        assert!(validate_locale("").is_err());
        assert!(validate_locale("not a locale!!").is_err());
        assert!(validate_locale("x-").is_err());
    }
}
