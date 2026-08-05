//! The NGSI-LD `Date` and `Time` lexical forms (ETSI GS CIM 009 clause 4.6.3).

use chrono::{NaiveDate, NaiveTime};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, From, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Error returned when an NGSI-LD value cannot be coerced into a typed variant.
#[derive(Debug, Error)]
pub enum NgsiLdValueError {
    /// The supplied date literal was invalid.
    #[error("invalid NGSI-LD Date value: {0}")]
    InvalidDate(String),
    /// The supplied time literal was invalid.
    #[error("invalid NGSI-LD Time value: {0}")]
    InvalidTime(String),
}

/// NGSI-LD `Date` lexical form as defined by clause 4.6.3.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, From, Into)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
#[into(String)]
pub struct NgsiLdDate(CompactString);

// Validating parser: the value is kept in its lexical form but only after
// `chrono` accepts it as a `YYYY-MM-DD` date, which no derive expresses.
impl FromStr for NgsiLdDate {
    type Err = NgsiLdValueError;

    fn from_str(value: &str) -> Result<NgsiLdDate, NgsiLdValueError> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| NgsiLdValueError::InvalidDate(value.to_string()))?;
        Ok(NgsiLdDate(CompactString::from(value)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for NgsiLdDate {
    type Error = NgsiLdValueError;

    fn try_from(value: String) -> Result<NgsiLdDate, NgsiLdValueError> {
        value.parse()
    }
}

/// NGSI-LD `Time` lexical form as defined by clause 4.6.3.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, From, Into)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
#[into(String)]
pub struct NgsiLdTime(CompactString);

// Validating parser: the value is kept in its lexical form but only after
// `chrono` accepts it as an `HH:MM:SS` time, which no derive expresses.
impl FromStr for NgsiLdTime {
    type Err = NgsiLdValueError;

    fn from_str(value: &str) -> Result<NgsiLdTime, NgsiLdValueError> {
        parse_ngsi_ld_time(value)?;
        Ok(NgsiLdTime(CompactString::from(value)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for NgsiLdTime {
    type Error = NgsiLdValueError;

    fn try_from(value: String) -> Result<NgsiLdTime, NgsiLdValueError> {
        value.parse()
    }
}

/// Validate an NGSI-LD `Time` literal: a UTC time-of-day with a mandatory `Z`.
///
/// # Errors
///
/// Returns [`NgsiLdValueError::InvalidTime`] when the suffix is missing or the
/// remaining text is not a valid time of day.
pub fn parse_ngsi_ld_time(value: &str) -> Result<(), NgsiLdValueError> {
    let Some(stripped) = value.strip_suffix('Z') else {
        return Err(NgsiLdValueError::InvalidTime(value.to_string()));
    };
    NaiveTime::parse_from_str(stripped, "%H:%M:%S%.f").map_err(|_| NgsiLdValueError::InvalidTime(value.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::attribute::date_time::{NgsiLdDate, NgsiLdTime};

    #[test]
    fn ngsi_ld_date_validates_iso_date() {
        let value = NgsiLdDate::try_from("2026-04-12".to_string()).expect("date");
        pretty_assertions::assert_eq!(value.to_string(), "2026-04-12");
    }

    #[test]
    fn ngsi_ld_time_validates_utc_time() {
        let value = NgsiLdTime::try_from("12:34:56.123Z".to_string()).expect("time");
        pretty_assertions::assert_eq!(value.to_string(), "12:34:56.123Z");
    }

    #[test]
    fn an_ngsi_ld_date_parses_from_a_borrowed_str() {
        let parsed: NgsiLdDate = "2026-08-04".parse().expect("valid date");
        pretty_assertions::assert_eq!(parsed.as_ref(), "2026-08-04");
        assert!("not-a-date".parse::<NgsiLdDate>().is_err());
    }

    #[test]
    fn an_ngsi_ld_time_parses_from_a_borrowed_str() {
        let parsed: NgsiLdTime = "12:34:56Z".parse().expect("valid time");
        pretty_assertions::assert_eq!(parsed.as_ref(), "12:34:56Z");
        assert!("not-a-time".parse::<NgsiLdTime>().is_err());
    }
}
