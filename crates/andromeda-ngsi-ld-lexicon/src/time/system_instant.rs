//! A system time the broker stamps on an attribute instance (clause 4.8).
//!
//! `createdAt` and `modifiedAt` are stamped by the broker from its clock. The
//! instant is held at the microsecond the store keeps and spelled as every
//! instant the broker emits is spelled.

use crate::time::{
    date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
    rendering::instant_text,
    resolution::to_microsecond,
};
use chrono::{DateTime, Utc};
use derive_more::Display;
use std::str::FromStr;

/// A broker-stamped instant, held at microsecond resolution.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{}", instant_text(*_0))]
pub struct SystemInstant(DateTime<Utc>);

impl SystemInstant {
    /// Stamp `instant`, truncated to the microsecond the store keeps.
    #[must_use]
    pub fn new(instant: DateTime<Utc>) -> SystemInstant {
        SystemInstant(to_microsecond(instant))
    }

    /// The instant.
    #[must_use]
    pub const fn get(self) -> DateTime<Utc> {
        self.0
    }
}

// A stored system time is read back under the clause 4.6.3 rules that wrote it.
impl FromStr for SystemInstant {
    type Err = NgsiLdDateTimeError;

    fn from_str(text: &str) -> Result<SystemInstant, NgsiLdDateTimeError> {
        text.parse::<NgsiLdDateTime>().map(|instant| SystemInstant(instant.get()))
    }
}

#[cfg(test)]
mod tests {
    use crate::time::system_instant::SystemInstant;
    use chrono::{TimeDelta, TimeZone, Utc};

    #[test]
    fn a_system_time_is_spelled_as_every_emitted_instant_is() {
        let second = Utc.with_ymd_and_hms(2026, 10, 1, 17, 39, 35).single().expect("a valid instant");
        pretty_assertions::assert_eq!(SystemInstant::new(second).to_string(), "2026-10-01T17:39:35Z");
        pretty_assertions::assert_eq!(
            SystemInstant::new(second + TimeDelta::nanoseconds(497_983_123)).to_string(),
            "2026-10-01T17:39:35.497983Z"
        );
    }

    #[test]
    fn a_stored_system_time_reads_back_to_its_own_text() {
        for text in ["2026-10-01T17:39:35.497983Z", "2026-10-01T17:39:35.500Z", "2026-10-01T17:39:35Z"] {
            let instant: SystemInstant = text.parse().expect("a stored system time");
            pretty_assertions::assert_eq!(instant.to_string(), text);
        }
    }

    #[test]
    fn a_stored_system_time_with_six_digits_reads_back_to_the_same_instant() {
        let fixed: SystemInstant = "2026-10-01T17:39:35.500000Z".parse().expect("a six-digit spelling");
        let minimal: SystemInstant = "2026-10-01T17:39:35.500Z".parse().expect("a minimal spelling");
        pretty_assertions::assert_eq!(fixed, minimal);
    }

    #[test]
    fn a_text_that_is_not_a_date_time_is_refused() {
        assert!("2026-10-01T17:39:35+00:00".parse::<SystemInstant>().is_err());
    }
}
