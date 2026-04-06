//! Quality value types for HTTP content negotiation.
//!
//! Portions of this file are derived from [Headers](https://github.com/hyperium/headers).
//! Licensed under the MIT License.
//! Copyright (c) 2014-2025 Sean McArthur.
//!
//! Implements quality factors as defined in
//! [RFC 9110 Section 12.4.2](https://www.rfc-editor.org/rfc/rfc9110#section-12.4.2)
//! (formerly [RFC 7231 Section 5.3.1](https://tools.ietf.org/html/rfc7231#section-5.3.1)).

use std::{cmp::Ordering, fmt};

/// Quality factor as defined in RFC 9110 Section 12.4.2 (formerly RFC 7231 Section 5.3.1).
///
/// Stored as an integer 0..=1000 representing q * 1000 (three decimal places).
/// Default quality is 1.0 (stored as 1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quality(u16);

impl Quality {
    /// Maximum quality value (q=1.0).
    pub const MAX: Quality = Quality(1000);

    /// Create a new quality value.
    ///
    /// Returns `None` if `value` exceeds 1000.
    pub const fn new(value: u16) -> Option<Quality> {
        if value > 1000 { None } else { Some(Quality(value)) }
    }

    /// Return the raw integer value (0..=1000).
    pub const fn value(&self) -> u16 {
        self.0
    }
}

// Manual Default impl: the default quality per RFC 9110 is 1.0 (1000), not 0.
// #[derive(Default)] would give Quality(0), which is incorrect.
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
            x => {
                let formatted = format!("{x:03}");
                let trimmed = formatted.trim_end_matches('0');
                write!(f, "q=0.{trimmed}")
            }
        }
    }
}

/// A value paired with an RFC 9110 quality factor.
///
/// Generic over the item type so it can be used with media types,
/// language tags, encodings, or any other quality-negotiated value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityValue<T> {
    value: T,
    quality: Quality,
}

impl<T> QualityValue<T> {
    /// Create a new quality value with an explicit quality.
    pub const fn new(value: T, quality: Quality) -> QualityValue<T> {
        QualityValue { value, quality }
    }

    /// Return a reference to the inner value.
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Return the quality factor.
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
        match self.quality.0 {
            1000 => Ok(()),
            _ => write!(f, "; {}", self.quality),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_valid_quality() {
        assert_eq!(Quality::new(500), Some(Quality(500)));
    }

    #[test]
    fn new_max_quality() {
        assert_eq!(Quality::new(1000), Some(Quality(1000)));
    }

    #[test]
    fn new_zero_quality() {
        assert_eq!(Quality::new(0), Some(Quality(0)));
    }

    #[test]
    fn new_rejects_above_1000() {
        assert_eq!(Quality::new(1001), None);
    }

    #[test]
    fn default_is_max() {
        assert_eq!(Quality::default(), Quality(1000));
    }

    #[test]
    fn display_max_omitted() {
        assert_eq!(Quality(1000).to_string(), "");
    }

    #[test]
    fn display_zero() {
        assert_eq!(Quality(0).to_string(), "q=0");
    }

    #[test]
    fn display_half() {
        assert_eq!(Quality(500).to_string(), "q=0.5");
    }

    #[test]
    fn display_precise() {
        assert_eq!(Quality(273).to_string(), "q=0.273");
    }

    #[test]
    fn display_trailing_zeros_trimmed() {
        assert_eq!(Quality(200).to_string(), "q=0.2");
    }

    #[test]
    fn ordering() {
        assert!(Quality(500) > Quality(273));
    }

    #[test]
    fn quality_value_default_quality() {
        let qv = QualityValue::new("foo", Quality::default());
        assert_eq!(qv.quality().value(), 1000);
    }

    #[test]
    fn quality_value_display_max() {
        assert_eq!(QualityValue::new("text/html", Quality::default()).to_string(), "text/html");
    }

    #[test]
    fn quality_value_display_with_q() {
        let qv = QualityValue::new("gzip", Quality(500));
        assert_eq!(qv.to_string(), "gzip; q=0.5");
    }

    #[test]
    fn quality_value_ordering_by_quality() {
        let high = QualityValue::new("a", Quality(900));
        let low = QualityValue::new("b", Quality(100));
        assert!(high > low);
    }
}
