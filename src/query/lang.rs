//! Parsing for the NGSI-LD `lang` query parameter (clause 6.3.5).

use crate::header::{LanguageRange, NgsiLdLanguage, Quality};
use langtag::LangTagBuf;
use std::cmp::Reverse;
use thiserror::Error;

/// Parsed NGSI-LD `lang` filter preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageFilter {
    preferences: Vec<LanguagePreference>,
}

impl LanguageFilter {
    /// Parse the NGSI-LD `lang` query parameter.
    pub fn parse(raw: &str) -> Result<LanguageFilter, LanguageFilterError> {
        let preferences = raw
            .split(',')
            .enumerate()
            .map(|(ordinal, item)| LanguagePreference::parse(item, ordinal))
            .collect::<Result<Vec<_>, _>>()?;

        if preferences.is_empty() {
            return Err(LanguageFilterError::Empty);
        }

        Ok(LanguageFilter { preferences })
    }

    /// Return preferences ordered by descending quality while preserving
    /// input order for equal-ranked entries.
    pub fn ordered(&self) -> Vec<&LanguagePreference> {
        let mut preferences = self.preferences.iter().filter(|preference| preference.quality.value() > 0).collect::<Vec<_>>();
        preferences.sort_by_key(|preference| (Reverse(preference.quality), preference.ordinal));
        preferences
    }
}

impl From<NgsiLdLanguage> for LanguageFilter {
    fn from(header: NgsiLdLanguage) -> LanguageFilter {
        let preferences = header
            .preferences()
            .iter()
            .enumerate()
            .map(|(ordinal, qv)| LanguagePreference {
                range: qv.value().clone(),
                quality: qv.quality(),
                ordinal,
            })
            .collect();
        LanguageFilter { preferences }
    }
}

/// A single ranked language preference from the `lang` query parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePreference {
    range: LanguageRange,
    quality: Quality,
    ordinal: usize,
}

impl LanguagePreference {
    fn parse(raw: &str, ordinal: usize) -> Result<LanguagePreference, LanguageFilterError> {
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

            quality = parse_quality_value(value.trim())?;
        }

        Ok(LanguagePreference { range, quality, ordinal })
    }

    /// Return the requested language range.
    pub const fn range(&self) -> &LanguageRange {
        &self.range
    }
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

/// Parse a quality value string like `0.9` into a [`Quality`].
fn parse_quality_value(raw: &str) -> Result<Quality, LanguageFilterError> {
    if raw.is_empty() {
        return Err(LanguageFilterError::InvalidQuality(raw.to_string()));
    }

    let (whole, fractional) = raw.split_once('.').map_or((raw, ""), |(whole, fractional)| (whole, fractional));
    let whole = match whole {
        "0" => 0_u16,
        "1" => 1_u16,
        _ => return Err(LanguageFilterError::InvalidQuality(raw.to_string())),
    };

    if fractional.len() > 3 || !fractional.chars().all(|character| character.is_ascii_digit()) {
        return Err(LanguageFilterError::InvalidQuality(raw.to_string()));
    }

    let mut padded = fractional.to_string();
    while padded.len() < 3 {
        padded.push('0');
    }

    let fractional = padded.parse::<u16>().map_err(|_| LanguageFilterError::InvalidQuality(raw.to_string()))?;

    if whole == 1 && fractional > 0 {
        return Err(LanguageFilterError::InvalidQuality(raw.to_string()));
    }

    Quality::new((whole * 1000) + fractional).ok_or_else(|| LanguageFilterError::InvalidQuality(raw.to_string()))
}

/// Typed parse errors for the NGSI-LD `lang` filter.
#[derive(Debug, Error)]
pub enum LanguageFilterError {
    /// The query parameter was empty.
    #[error("The `lang` query parameter cannot be empty")]
    Empty,
    /// One of the comma-delimited preferences was empty.
    #[error("The `lang` query parameter contains an empty language preference")]
    EmptyPreference,
    /// A malformed parameter segment was found.
    #[error("Malformed language preference parameter `{0}`")]
    MalformedParameter(String),
    /// A parameter other than `q` was provided.
    #[error("Unsupported language preference parameter `{0}`")]
    UnsupportedParameter(String),
    /// A `q` value was syntactically invalid.
    #[error("Invalid language preference quality value `{0}`")]
    InvalidQuality(String),
    /// A language tag was invalid.
    #[error("Invalid language tag `{value}`")]
    InvalidLanguageTag {
        /// The original invalid input.
        value: String,
        /// The source parser error.
        #[source]
        source: langtag::InvalidLangTag<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::{LanguageFilter, LanguageFilterError};
    use crate::header::LanguageRange;

    #[test]
    fn parses_ranked_preferences_in_quality_order() {
        let filter = LanguageFilter::parse("fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5").expect("language filter");
        let ordered = filter.ordered();

        assert!(matches!(ordered[0].range(), LanguageRange::Tag(tag) if tag.to_string() == "fr-CH"));
        assert!(matches!(ordered[1].range(), LanguageRange::Tag(tag) if tag.to_string() == "fr"));
        assert!(matches!(ordered[2].range(), LanguageRange::Tag(tag) if tag.to_string() == "en"));
        assert!(matches!(ordered[3].range(), LanguageRange::Wildcard));
    }

    #[test]
    fn rejects_invalid_quality_values() {
        let error = LanguageFilter::parse("fr;q=1.1").expect_err("invalid quality");
        assert!(matches!(error, LanguageFilterError::InvalidQuality(value) if value == "1.1"));
    }

    #[test]
    fn rejects_unknown_parameters() {
        let error = LanguageFilter::parse("fr;level=1").expect_err("unknown parameter");
        assert!(matches!(error, LanguageFilterError::UnsupportedParameter(name) if name == "level"));
    }

    #[test]
    fn filters_out_zero_quality_preferences_from_ordered_results() {
        let filter = LanguageFilter::parse("de;q=0,en;q=0.8,*;q=0.5").expect("language filter");
        let ordered = filter.ordered();

        assert_eq!(ordered.len(), 2);
        assert!(matches!(ordered[0].range(), LanguageRange::Tag(tag) if tag.to_string() == "en"));
        assert!(matches!(ordered[1].range(), LanguageRange::Wildcard));
    }

    #[test]
    fn preserves_input_order_for_equal_quality_preferences() {
        let filter = LanguageFilter::parse("de,en,fr").expect("language filter");
        let ordered = filter.ordered();
        assert_eq!(ordered.len(), 3);
        // All have implicit q=1.0, so input order must be preserved
        assert!(matches!(ordered[0].range(), LanguageRange::Tag(tag) if tag.to_string() == "de"));
        assert!(matches!(ordered[1].range(), LanguageRange::Tag(tag) if tag.to_string() == "en"));
        assert!(matches!(ordered[2].range(), LanguageRange::Tag(tag) if tag.to_string() == "fr"));
    }

    #[test]
    fn defaults_missing_quality_to_one() {
        let filter = LanguageFilter::parse("fr").expect("language filter");
        let ordered = filter.ordered();

        assert_eq!(ordered.len(), 1);
        assert!(matches!(ordered[0].range(), LanguageRange::Tag(tag) if tag.to_string() == "fr"));
    }
}
