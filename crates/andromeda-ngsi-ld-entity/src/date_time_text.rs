//! A client-written `DateTime` member, kept as the text the client wrote.
//!
//! Clause 4.6.4 makes the broker "preserve the representation of the content of
//! the values" it is given, and the stored document keeps an `observedAt` or an
//! `expiresAt` (clause 4.22) in the spelling of the request. The text is what
//! is written back; the instant parsed from it is what expiry and recency
//! comparisons read.

use andromeda_ngsi_ld_lexicon::rfc3339::sub_second;
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// A text that is not an RFC 3339 date-time.
///
/// The text itself is deliberately not part of the error: refusals reach
/// clients, and the broker never echoes raw input back.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the value is not an RFC 3339 date-time")]
pub struct DateTimeTextError;

/// The verbatim text of a `DateTime` member and the instant it names.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("{text}")]
pub struct DateTimeText {
    /// The spelling as written.
    text: CompactString,
    /// The UTC instant the spelling names.
    instant: DateTime<Utc>,
}

impl DateTimeText {
    /// A `DateTime` the broker writes on the client's behalf from an instant
    /// it holds (the Table 6.5.3.4-1 `observedAt` request parameter), spelled
    /// the one way the broker renders an instant: UTC, `Z`, and exactly the
    /// sub-second precision the instant carries.
    #[must_use]
    pub fn from_instant(instant: DateTime<Utc>) -> DateTimeText {
        DateTimeText {
            text: CompactString::from(sub_second(instant)),
            instant,
        }
    }

    /// The text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The instant the text names.
    #[must_use]
    pub const fn instant(&self) -> DateTime<Utc> {
        self.instant
    }
}

impl FromStr for DateTimeText {
    type Err = DateTimeTextError;

    fn from_str(text: &str) -> Result<DateTimeText, DateTimeTextError> {
        let instant = DateTime::parse_from_rfc3339(text).map_err(|_| DateTimeTextError)?.with_timezone(&Utc);
        Ok(DateTimeText {
            text: CompactString::from(text),
            instant,
        })
    }
}

impl TryFrom<String> for DateTimeText {
    type Error = DateTimeTextError;

    fn try_from(text: String) -> Result<DateTimeText, DateTimeTextError> {
        let instant = DateTime::parse_from_rfc3339(&text).map_err(|_| DateTimeTextError)?.with_timezone(&Utc);
        Ok(DateTimeText {
            text: CompactString::from(text),
            instant,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::date_time_text::{DateTimeText, DateTimeTextError};
    use chrono::{TimeZone, Utc};

    #[test]
    fn the_written_spelling_is_kept_beside_the_instant() {
        let text: DateTimeText = "2026-03-01T10:00:00.50+01:00".parse().expect("RFC 3339");
        pretty_assertions::assert_eq!(text.as_str(), "2026-03-01T10:00:00.50+01:00");
        pretty_assertions::assert_eq!(
            text.instant(),
            Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).single().expect("a valid instant") + chrono::Duration::milliseconds(500)
        );
    }

    #[test]
    fn a_text_that_is_not_rfc_3339_is_refused() {
        pretty_assertions::assert_eq!("yesterday".parse::<DateTimeText>(), Err(DateTimeTextError));
    }
}
