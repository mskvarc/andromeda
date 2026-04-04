use language_tags::{LanguageTag, ParseError, ValidationError};
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
        let mut preferences = self
            .preferences
            .iter()
            .filter(|preference| preference.quality.as_millis() > 0)
            .collect::<Vec<_>>();
        preferences.sort_by_key(|preference| (Reverse(preference.quality.as_millis()), preference.ordinal));
        preferences
    }
}

/// A single ranked language preference from the `lang` query parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePreference {
    range: LanguageRange,
    quality: QualityValue,
    ordinal: usize,
}

impl LanguagePreference {
    fn parse(raw: &str, ordinal: usize) -> Result<LanguagePreference, LanguageFilterError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(LanguageFilterError::EmptyPreference);
        }

        let mut segments = trimmed.split(';');
        let range = LanguageRange::parse(segments.next().ok_or(LanguageFilterError::EmptyPreference)?.trim())?;
        let mut quality = QualityValue::preferred();

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

            quality = QualityValue::parse(value.trim())?;
        }

        Ok(LanguagePreference { range, quality, ordinal })
    }

    /// Return the requested language range.
    pub fn range(&self) -> &LanguageRange {
        &self.range
    }
}

/// One NGSI-LD language range preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageRange {
    /// Wildcard preference matching any available language.
    Wildcard,
    /// A typed BCP47 language range.
    Tag(LanguageTag),
}

impl LanguageRange {
    fn parse(raw: &str) -> Result<LanguageRange, LanguageFilterError> {
        if raw == "*" {
            return Ok(LanguageRange::Wildcard);
        }

        let parsed = LanguageTag::parse(raw).map_err(|error| LanguageFilterError::InvalidLanguageTag {
            value: raw.to_string(),
            source: error,
        })?;
        let canonical = parsed.canonicalize().unwrap_or(parsed);

        Ok(LanguageRange::Tag(canonical))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct QualityValue(u16);

impl QualityValue {
    const fn preferred() -> QualityValue {
        QualityValue(1000)
    }

    fn parse(raw: &str) -> Result<QualityValue, LanguageFilterError> {
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

        Ok(QualityValue((whole * 1000) + fractional))
    }

    fn as_millis(self) -> u16 {
        self.0
    }
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
        source: ParseError,
    },
    /// A stored language tag could not be canonicalized.
    #[error("Invalid language tag `{value}`")]
    InvalidStoredLanguageTag {
        /// The original invalid input.
        value: String,
        /// The source validator error.
        #[source]
        source: ValidationError,
    },
}

#[cfg(test)]
mod tests {
    use super::{LanguageFilter, LanguageFilterError, LanguageRange};

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
