//! A system time the broker stamps on an attribute instance (clause 4.8).
//!
//! `createdAt` and `modifiedAt` are written by the broker, never by a client,
//! at microsecond precision: the stored document always carries six fractional
//! digits and the `Z` designator, so the instant alone determines the text.

use chrono::{DateTime, SecondsFormat, SubsecRound, Utc};
use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// A stored system time that is not an RFC 3339 date-time.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the system time is not an RFC 3339 date-time")]
pub struct SystemInstantError;

/// A broker-stamped instant, held at microsecond precision.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{}", _0.to_rfc3339_opts(SecondsFormat::Micros, true))]
pub struct SystemInstant(DateTime<Utc>);

impl SystemInstant {
    /// Stamp `instant`, truncated to the microsecond precision the stored
    /// document carries.
    #[must_use]
    pub fn new(instant: DateTime<Utc>) -> SystemInstant {
        SystemInstant(instant.trunc_subsecs(6))
    }

    /// The instant.
    #[must_use]
    pub const fn get(self) -> DateTime<Utc> {
        self.0
    }
}

impl FromStr for SystemInstant {
    type Err = SystemInstantError;

    fn from_str(text: &str) -> Result<SystemInstant, SystemInstantError> {
        DateTime::parse_from_rfc3339(text)
            .map(|instant| SystemInstant::new(instant.with_timezone(&Utc)))
            .map_err(|_| SystemInstantError)
    }
}

#[cfg(test)]
mod tests {
    use crate::system_instant::SystemInstant;
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_system_time_renders_six_fractional_digits() {
        let instant = SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 1, 17, 39, 35).single().expect("a valid instant"));
        pretty_assertions::assert_eq!(instant.to_string(), "2026-10-01T17:39:35.000000Z");
    }

    #[test]
    fn a_stored_system_time_reads_back_to_its_own_text() {
        let instant: SystemInstant = "2026-10-01T17:39:35.497983Z".parse().expect("RFC 3339");
        pretty_assertions::assert_eq!(instant.to_string(), "2026-10-01T17:39:35.497983Z");
    }
}
