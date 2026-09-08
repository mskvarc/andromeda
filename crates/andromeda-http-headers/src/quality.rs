//! SPDX-FileCopyrightText: 2014-2025 Sean `McArthur`
//! SPDX-License-Identifier: MIT
//!
//! Quality factors for HTTP content negotiation.
//!
//! Portions of this file are derived from the disabled quality utilities in
//! <https://github.com/hyperium/headers>.
//! This file, including local modifications, is distributed under the MIT License.
//!
//! Implements quality factors as defined in
//! [RFC 9110 Section 12.4.2](https://www.rfc-editor.org/rfc/rfc9110#section-12.4.2)
//! (formerly [RFC 7231 Section 5.3.1](https://tools.ietf.org/html/rfc7231#section-5.3.1)).

use std::{cmp::Ordering, fmt};
use thiserror::Error;

/// Quality factor as defined in RFC 9110 Section 12.4.2.
///
/// Stored as an integer 0..=1000 representing q * 1000 (three decimal places),
/// so comparison and ordering are exact where floating point would not be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quality(u16);

impl Quality {
    /// Maximum quality value (`q=1.0`).
    pub const MAX: Quality = Quality(1000);

    /// Create a new quality value.
    ///
    /// Returns `None` if `value` exceeds 1000.
    #[must_use]
    pub const fn new(value: u16) -> Option<Quality> {
        if value > 1000 { None } else { Some(Quality(value)) }
    }

    /// The raw integer value (0..=1000).
    #[must_use]
    pub const fn value(&self) -> u16 {
        self.0
    }

    /// Parse a decimal quality string like `"0.9"`.
    ///
    /// Accepts values in the range `0` to `1` with at most 3 decimal places.
    /// Uses precise decimal arithmetic, so `0.1` is exactly 100 rather than
    /// whatever the nearest `f32` happens to be.
    ///
    /// # Errors
    ///
    /// Returns [`QualityParseError`] if the input is empty, non-numeric,
    /// out of range, or has more than 3 decimal places.
    pub fn parse_decimal(raw: &str) -> Result<Quality, QualityParseError> {
        if raw.is_empty() {
            return Err(QualityParseError::Empty);
        }

        let (whole, fractional) = raw.split_once('.').map_or((raw, ""), |(whole, fractional)| (whole, fractional));

        let whole = match whole {
            "0" => 0_u16,
            "1" => 1_u16,
            _ => return Err(QualityParseError::Invalid(raw.to_string())),
        };

        if fractional.len() > 3 || !fractional.chars().all(|character| character.is_ascii_digit()) {
            return Err(QualityParseError::Invalid(raw.to_string()));
        }

        let fractional_value = if fractional.is_empty() {
            0_u16
        } else {
            let mut padded = fractional.to_string();
            while padded.len() < 3 {
                padded.push('0');
            }
            padded.parse::<u16>().map_err(|_| QualityParseError::Invalid(raw.to_string()))?
        };

        if whole == 1 && fractional_value > 0 {
            return Err(QualityParseError::Invalid(raw.to_string()));
        }

        Quality::new((whole * 1000) + fractional_value).ok_or_else(|| QualityParseError::Invalid(raw.to_string()))
    }
}

/// Errors from parsing a quality value string.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum QualityParseError {
    /// The input string was empty.
    #[error("quality value cannot be empty")]
    Empty,
    /// The input was not a valid quality value: out of range, too many decimal
    /// places, or non-numeric.
    #[error("invalid quality value `{0}`")]
    Invalid(String),
}

// The RFC 9110 default is 1.0, so the derived `Quality(0)` would be wrong.
impl Default for Quality {
    fn default() -> Quality {
        Quality::MAX
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            1000 => Ok(()),
            0 => f.write_str("q=0"),
            thousandths => {
                let formatted = format!("{thousandths:03}");
                let trimmed = formatted.trim_end_matches('0');
                write!(f, "q=0.{trimmed}")
            }
        }
    }
}

/// A value paired with an RFC 9110 quality factor.
///
/// Generic over the item type so it serves media types, language tags,
/// encodings, or any other quality-negotiated value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityValue<T> {
    /// The negotiated item itself.
    value: T,
    /// How strongly the client asked for it.
    quality: Quality,
}

impl<T> QualityValue<T> {
    /// Pair a value with an explicit quality.
    pub const fn new(value: T, quality: Quality) -> QualityValue<T> {
        QualityValue { value, quality }
    }

    /// The inner value.
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The quality factor.
    pub const fn quality(&self) -> Quality {
        self.quality
    }
}

impl<T: PartialEq> PartialOrd for QualityValue<T> {
    fn partial_cmp(&self, other: &QualityValue<T>) -> Option<Ordering> {
        self.quality.partial_cmp(&other.quality)
    }
}

impl<T: fmt::Display> fmt::Display for QualityValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.value, f)?;
        match self.quality.value() {
            1000 => Ok(()),
            _ => write!(f, "; {}", self.quality),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::quality::{Quality, QualityParseError, QualityValue};

    #[test]
    fn a_quality_within_range_is_accepted_and_one_above_is_not() {
        pretty_assertions::assert_eq!(Quality::new(500).map(|quality| quality.value()), Some(500));
        pretty_assertions::assert_eq!(Quality::new(1000).map(|quality| quality.value()), Some(1000));
        pretty_assertions::assert_eq!(Quality::new(0).map(|quality| quality.value()), Some(0));
        pretty_assertions::assert_eq!(Quality::new(1001), None);
    }

    #[test]
    fn the_default_quality_is_the_maximum() {
        pretty_assertions::assert_eq!(Quality::default(), Quality::MAX);
    }

    #[test]
    fn rendering_omits_the_maximum_and_trims_trailing_zeros() {
        pretty_assertions::assert_eq!(Quality::MAX.to_string(), "");
        pretty_assertions::assert_eq!(Quality::new(0).expect("in range").to_string(), "q=0");
        pretty_assertions::assert_eq!(Quality::new(500).expect("in range").to_string(), "q=0.5");
        pretty_assertions::assert_eq!(Quality::new(273).expect("in range").to_string(), "q=0.273");
        pretty_assertions::assert_eq!(Quality::new(200).expect("in range").to_string(), "q=0.2");
    }

    #[test]
    fn qualities_order_by_magnitude() {
        assert!(Quality::new(500) > Quality::new(273));
    }

    #[test]
    fn a_decimal_quality_parses_to_exact_thousandths() {
        pretty_assertions::assert_eq!(Quality::parse_decimal("0.5"), Ok(Quality(500)));
        pretty_assertions::assert_eq!(Quality::parse_decimal("0"), Ok(Quality(0)));
        pretty_assertions::assert_eq!(Quality::parse_decimal("1"), Ok(Quality::MAX));
        pretty_assertions::assert_eq!(Quality::parse_decimal("1.0"), Ok(Quality::MAX));
        pretty_assertions::assert_eq!(Quality::parse_decimal("0.273"), Ok(Quality(273)));
    }

    #[test]
    fn a_quality_outside_the_grammar_is_rejected() {
        pretty_assertions::assert_eq!(Quality::parse_decimal("1.1"), Err(QualityParseError::Invalid("1.1".to_string())));
        pretty_assertions::assert_eq!(Quality::parse_decimal("0.1234"), Err(QualityParseError::Invalid("0.1234".to_string())));
        pretty_assertions::assert_eq!(Quality::parse_decimal("abc"), Err(QualityParseError::Invalid("abc".to_string())));
        pretty_assertions::assert_eq!(Quality::parse_decimal(""), Err(QualityParseError::Empty));
    }

    #[test]
    fn a_quality_value_carries_its_factor_and_renders_it_only_below_the_maximum() {
        let default = QualityValue::new("text/html", Quality::default());
        pretty_assertions::assert_eq!(default.quality().value(), 1000);
        pretty_assertions::assert_eq!(default.to_string(), "text/html");

        let ranked = QualityValue::new("gzip", Quality::new(500).expect("in range"));
        pretty_assertions::assert_eq!(ranked.to_string(), "gzip; q=0.5");
    }

    #[test]
    fn quality_values_order_by_their_factor() {
        let high = QualityValue::new("a", Quality::new(900).expect("in range"));
        let low = QualityValue::new("b", Quality::new(100).expect("in range"));
        assert!(high > low);
    }
}
