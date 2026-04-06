//! Wire format parsing for the NGSI-LD `lang` query parameter (clause 6.3.5).
//!
//! Parses the comma-separated BCP 47 language preference format
//! (`"fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5"`) into a [`LanguageFilter`].

use crate::http::model::{
    language::{LanguageFilter, LanguageFilterError, LanguageRange},
    quality::{Quality, QualityValue},
};
use langtag::LangTagBuf;

/// Parse the NGSI-LD `lang` query parameter value into a [`LanguageFilter`].
///
/// The wire format is a comma-separated list of BCP 47 language ranges,
/// each optionally followed by a `;q=` quality parameter (NGSI-LD clause 6.3.5).
///
/// # Errors
///
/// Returns [`LanguageFilterError`] if the input is empty, contains invalid
/// language tags, or has malformed quality parameters.
pub fn parse(raw: &str) -> Result<LanguageFilter, LanguageFilterError> {
    let preferences = raw.split(',').map(parse_preference).collect::<Result<Vec<_>, _>>()?;

    LanguageFilter::new(preferences)
}

/// Parse a single language preference segment like `"fr;q=0.9"`.
fn parse_preference(raw: &str) -> Result<QualityValue<LanguageRange>, LanguageFilterError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(LanguageFilterError::EmptyPreference);
    }

    let mut segments = trimmed.split(';');
    let range_str = segments.next().ok_or(LanguageFilterError::EmptyPreference)?.trim();
    let range = parse_language_range(range_str)?;
    let mut quality = Quality::default();

    for parameter in segments {
        let parameter = parameter.trim();
        if parameter.is_empty() {
            return Err(LanguageFilterError::MalformedParameter(trimmed.to_string()));
        }

        let (name, value) = parameter
            .split_once('=')
            .ok_or_else(|| LanguageFilterError::MalformedParameter(parameter.to_string()))?;

        if name.trim() != "q" {
            return Err(LanguageFilterError::UnsupportedParameter(name.trim().to_string()));
        }

        quality = Quality::parse_decimal(value.trim()).map_err(|_| LanguageFilterError::InvalidQuality(value.trim().to_string()))?;
    }

    Ok(QualityValue::new(range, quality))
}

/// Parse a language range string into a [`LanguageRange`].
fn parse_language_range(raw: &str) -> Result<LanguageRange, LanguageFilterError> {
    if raw == "*" {
        return Ok(LanguageRange::Wildcard);
    }

    let tag = LangTagBuf::new(raw.to_string()).map_err(|error| LanguageFilterError::InvalidLanguageTag {
        value: raw.to_string(),
        source: error,
    })?;

    Ok(LanguageRange::Tag(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_tag() {
        let filter = parse("fr").expect("valid");
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].value().to_string(), "fr");
        assert_eq!(ordered[0].quality().value(), 1000);
    }

    #[test]
    fn parses_ranked_preferences() {
        let filter = parse("fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5").expect("valid");
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 4);
        assert_eq!(ordered[0].value().to_string(), "fr-CH");
        assert_eq!(ordered[1].value().to_string(), "fr");
        assert_eq!(ordered[2].value().to_string(), "en");
        assert_eq!(ordered[3].value().to_string(), "*");
    }

    #[test]
    fn rejects_empty_input() {
        let error = parse("").expect_err("empty");
        assert!(matches!(error, LanguageFilterError::EmptyPreference));
    }

    #[test]
    fn rejects_empty_segment() {
        let error = parse("fr,,en").expect_err("empty segment");
        assert!(matches!(error, LanguageFilterError::EmptyPreference));
    }

    #[test]
    fn rejects_invalid_quality() {
        let error = parse("fr;q=1.1").expect_err("invalid quality");
        assert!(matches!(error, LanguageFilterError::InvalidQuality(ref v) if v == "1.1"));
    }

    #[test]
    fn rejects_unknown_parameter() {
        let error = parse("fr;level=1").expect_err("unknown param");
        assert!(matches!(error, LanguageFilterError::UnsupportedParameter(ref n) if n == "level"));
    }

    #[test]
    fn rejects_malformed_parameter() {
        let error = parse("fr;q").expect_err("malformed");
        assert!(matches!(error, LanguageFilterError::MalformedParameter(_)));
    }

    #[test]
    fn rejects_invalid_language_tag() {
        let error = parse("123").expect_err("invalid tag");
        assert!(matches!(error, LanguageFilterError::InvalidLanguageTag { .. }));
    }
}
