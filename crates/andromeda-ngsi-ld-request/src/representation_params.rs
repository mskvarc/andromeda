//! The `format` and `options` query parameters that pick an entity's
//! representation (clause 4.5.1).
//!
//! `format` is the current selector and `options` its legacy predecessor. Both
//! the GET-query parameter set ([`crate::query::entity::EntityQueryParameters`])
//! and the single-entity GET parameter set ([`crate::entity::GetEntityParams`])
//! carry them with identical grammar, so resolving them into a representation
//! and rejecting values outside the grammar live here once rather than in each.

use crate::error::ParameterError;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;

/// The `options` tokens the broker defines for an entity representation.
const VALID_OPTION_TOKENS: [&str; 5] = ["keyValues", "simplified", "sysAttrs", "concise", "normalized"];

/// The `format` values the broker defines for an entity representation.
const VALID_FORMAT_VALUES: [&str; 4] = ["normalized", "keyValues", "simplified", "concise"];

/// Resolve the [`ResponseFormat`] from the `format` and `options` parameters.
///
/// Clause 4.5.1 makes `format` the current selector and `options` its legacy
/// predecessor; where both are present `format` wins, so a recognised `format`
/// returns before `options` is consulted. `keyValues` and `simplified` are the
/// two spellings of the simplified representation. Option values are matched as
/// whole comma-separated tokens, never as substrings, so `options=mykeyValues`
/// is not mistaken for `keyValues`.
pub fn resolve_response_format(format: Option<&str>, options: Option<&str>) -> ResponseFormat {
    if let Some(f) = format {
        match f {
            "keyValues" | "simplified" => return ResponseFormat::Simplified,
            "concise" => return ResponseFormat::Concise,
            "normalized" => return ResponseFormat::Normalized,
            // `&str` is unbounded, so a catch-all is the only way to close this
            // match. An unrecognised `format` is a rejected request once
            // validation runs; falling through keeps resolution total.
            _ => {}
        }
    }
    if let Some(opts) = options {
        for token in opts.split(',').map(str::trim) {
            match token {
                "keyValues" | "simplified" => return ResponseFormat::Simplified,
                "concise" => return ResponseFormat::Concise,
                "normalized" => return ResponseFormat::Normalized,
                // As above: an unrecognised token does not decide the mode.
                _ => {}
            }
        }
    }
    ResponseFormat::Normalized
}

/// Reject a `format` or `options` value outside the clause 4.5.1 grammar.
///
/// An unknown value is a client error, not something to silently ignore: clause
/// 6.3.1 has a broker refuse a request it does not fully understand. `format`
/// must name a defined representation and each comma-separated `options` token
/// must be one the broker defines.
pub fn validate_representation_params(format: Option<&str>, options: Option<&str>) -> Result<(), ParameterError> {
    if let Some(fmt) = format
        && !VALID_FORMAT_VALUES.contains(&fmt)
    {
        return Err(ParameterError::FormatInvalid(fmt.to_string()));
    }
    if let Some(opts) = options {
        for opt in opts.split(',').map(str::trim) {
            if !VALID_OPTION_TOKENS.contains(&opt) {
                return Err(ParameterError::OptionsInvalid(opt.to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_wins_over_a_conflicting_options() {
        pretty_assertions::assert_eq!(resolve_response_format(Some("normalized"), Some("keyValues")), ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(resolve_response_format(Some("concise"), Some("keyValues")), ResponseFormat::Concise);
        pretty_assertions::assert_eq!(resolve_response_format(Some("keyValues"), Some("concise")), ResponseFormat::Simplified);
    }

    #[test]
    fn options_decides_the_mode_when_no_format_is_given() {
        pretty_assertions::assert_eq!(resolve_response_format(None, Some("keyValues")), ResponseFormat::Simplified);
        pretty_assertions::assert_eq!(resolve_response_format(None, Some("simplified")), ResponseFormat::Simplified);
        pretty_assertions::assert_eq!(resolve_response_format(None, Some("concise")), ResponseFormat::Concise);
    }

    #[test]
    fn the_default_representation_is_normalized() {
        pretty_assertions::assert_eq!(resolve_response_format(None, None), ResponseFormat::Normalized);
    }

    #[test]
    fn a_substring_of_a_token_is_not_that_token() {
        pretty_assertions::assert_eq!(resolve_response_format(None, Some("mykeyValues")), ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(resolve_response_format(None, Some("conciseness")), ResponseFormat::Normalized);
    }

    #[test]
    fn a_sysattrs_token_alongside_a_mode_still_resolves_the_mode() {
        pretty_assertions::assert_eq!(resolve_response_format(None, Some("keyValues,sysAttrs")), ResponseFormat::Simplified);
    }

    #[test]
    fn validation_rejects_an_unknown_format() {
        assert!(matches!(
            validate_representation_params(Some("bananas"), None),
            Err(ParameterError::FormatInvalid(_))
        ));
    }

    #[test]
    fn validation_rejects_an_unknown_options_token() {
        assert!(matches!(
            validate_representation_params(None, Some("keyValues,bogus")),
            Err(ParameterError::OptionsInvalid(_))
        ));
    }

    #[test]
    fn validation_accepts_defined_values() {
        assert!(validate_representation_params(Some("concise"), Some("sysAttrs")).is_ok());
        assert!(validate_representation_params(Some("normalized"), Some("keyValues,sysAttrs")).is_ok());
    }
}
