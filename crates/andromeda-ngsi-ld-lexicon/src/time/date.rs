//! The NGSI-LD `Date` (ETSI GS CIM 009 v1.9.1 clause 4.6.3): a calendar date in its one spelling.
//!
//! Clause 4.6.3 writes a `Date` as `YYYY-MM-DD`, every component present. That
//! spelling is fixed-width and zero-padded, so byte-wise (`COLLATE "C"`) order
//! over the stored text is chronological order: clause 4.9's `<`, `>`, `<=` and
//! `>=` over a `Date` are answered by a plain string comparison and by the
//! B-tree index over it. A lenient spelling such as `2026-4-2` names a real day
//! but is not the clause's form and sorts nowhere near its neighbours, so it is
//! refused.

use crate::time::{
    lexical::{self, LexicalError},
    rendering::date_text,
};
use chrono::NaiveDate;
use derive_more::{AsRef, Deref, Display, From};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Why a text is not an NGSI-LD `Date`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NgsiLdDateError {
    /// The text is not written in the clause 4.6.3 `Date` form.
    #[error("NGSI-LD Date value {0}")]
    Lexical(#[from] LexicalError),
    /// The text is written in the form but names no calendar day.
    #[error("NGSI-LD Date value names no calendar day")]
    OutOfRange,
}

/// An NGSI-LD `Date` in its `YYYY-MM-DD` spelling.
///
/// Two values compare through [`Ord`] in chronological order, and their
/// spellings compare byte-wise in the same order. [`From<NaiveDate>`] is total:
/// every calendar date a four-digit year reaches has a spelling, so only text
/// can fail to be one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Display, From, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[display("{}", date_text(*_0))]
pub struct NgsiLdDate(NaiveDate);

impl NgsiLdDate {
    /// The calendar date this value names.
    #[must_use]
    pub const fn get(self) -> NaiveDate {
        self.0
    }
}

// Validating parser: the clause 4.6.3 grammar and the calendar check are steps
// no derive expresses, and `derive_more::FromStr` would delegate to
// `NaiveDate`'s own reader, which admits unpadded components.
impl FromStr for NgsiLdDate {
    type Err = NgsiLdDateError;

    fn from_str(text: &str) -> Result<NgsiLdDate, NgsiLdDateError> {
        let date = lexical::date(text)?;
        NaiveDate::from_ymd_opt(date.year, date.month, date.day)
            .map(NgsiLdDate)
            .ok_or(NgsiLdDateError::OutOfRange)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter is what `#[serde(try_from = "String")]` requires.
impl TryFrom<String> for NgsiLdDate {
    type Error = NgsiLdDateError;

    fn try_from(text: String) -> Result<NgsiLdDate, NgsiLdDateError> {
        text.parse()
    }
}

// `derive_more::Into` would yield `Into<NaiveDate>`, the inner type. What
// `#[serde(into = "String")]` needs is the spelling, which no derive produces
// from a `chrono` value.
impl From<NgsiLdDate> for String {
    fn from(date: NgsiLdDate) -> String {
        date.to_string()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::time::{
        date::{NgsiLdDate, NgsiLdDateError},
        lexical::LexicalError,
    };

    #[test]
    fn a_date_round_trips_unchanged() {
        let parsed: NgsiLdDate = "2026-04-12".parse().expect("a Date");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-04-12");
    }

    #[test]
    fn an_unpadded_or_reduced_spelling_is_malformed() {
        for text in ["2026-4-2", "2026-04", "26-04-12", "2026-04-12T00:00:00Z", "not-a-date"] {
            pretty_assertions::assert_eq!(text.parse::<NgsiLdDate>(), Err(NgsiLdDateError::Lexical(LexicalError::Malformed)), "{text}");
        }
    }

    #[test]
    fn a_day_the_calendar_does_not_have_is_out_of_range() {
        for text in ["2026-13-01", "2026-02-30", "2026-00-10"] {
            pretty_assertions::assert_eq!(text.parse::<NgsiLdDate>(), Err(NgsiLdDateError::OutOfRange), "{text}");
        }
    }

    #[test]
    fn byte_wise_order_over_the_spelling_is_chronological_order() {
        let ordered = ["0001-01-01", "1970-01-01", "1970-01-02", "1970-02-01", "1999-12-31", "2000-01-01", "2026-04-12"];
        let parsed: Vec<NgsiLdDate> = ordered.iter().map(|value| value.parse().expect("a Date")).collect();
        for window in parsed.windows(2) {
            let (earlier, later) = (window[0], window[1]);
            assert!(earlier < later, "{earlier} must precede {later} chronologically");
            assert!(earlier.to_string() < later.to_string(), "{earlier} must precede {later} byte-wise as well");
        }
    }

    #[test]
    fn an_owned_string_parses_through_the_try_from_adapter() {
        let parsed = NgsiLdDate::try_from("2026-08-04".to_owned()).expect("an owned Date");
        pretty_assertions::assert_eq!(String::from(parsed), "2026-08-04");
    }
}
