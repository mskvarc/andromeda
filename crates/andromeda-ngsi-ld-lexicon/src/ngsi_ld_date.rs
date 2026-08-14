//! The NGSI-LD `Date` lexical form in the one rendering the broker stores, orders and compares (ETSI GS CIM 009 clause 4.6.3).
//!
//! Clause 4.6.3 defines `Date` as an ISO 8601 [Extended Format] `full-date`, whose canonical
//! spelling `YYYY-MM-DD` is fixed-width and zero-padded. That is the property the
//! broker depends on: byte-wise (`COLLATE "C"`) lexical order over the stored
//! text is chronological order, so clause 4.9's `<`, `>`, `<=` and `>=` over a
//! `Date` value are answered by a plain string comparison and by the B-tree index
//! over it, with no parsing in the statement.
//!
//! A value is therefore parsed and re-rendered rather than kept verbatim: a
//! lenient spelling such as `2026-1-1` names a real day but sorts nowhere near
//! the padded form of its neighbours.

use chrono::NaiveDate;
use derive_more::{AsRef, Deref, Display, From};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// The canonical rendering, which is also the only spelling accepted back.
const CANONICAL_FORMAT: &str = "%Y-%m-%d";

/// Errors raised when a `Date` value cannot be accepted in canonical form.
#[derive(Debug, Error)]
pub enum NgsiLdDateError {
    /// The text is not an RFC 3339 `full-date`.
    #[error("NGSI-LD Date value is not an RFC 3339 full-date: {value}")]
    Malformed {
        /// The rejected text.
        value: String,
    },
}

/// An NGSI-LD `Date` in canonical `YYYY-MM-DD` form.
///
/// Two values compare through [`Ord`] in chronological order, and their canonical
/// renderings compare byte-wise in the same order.
///
/// [`From<NaiveDate>`] is total rather than fallible: every calendar date has a
/// canonical rendering, so only *text* can fail to be one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Display, From, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[display("{}", _0.format(CANONICAL_FORMAT))]
pub struct NgsiLdDate(NaiveDate);

impl NgsiLdDate {
    /// The calendar date this value names.
    pub const fn get(self) -> NaiveDate {
        self.0
    }
}

// Validating parser: `chrono` accepts spellings RFC 3339 does not — `%m` and
// `%d` read one digit as readily as two — so the round trip below is what pins
// the grammar to the single canonical form. `derive_more::FromStr` would delegate
// to `NaiveDate`'s own parser and admit all of them.
//
// Normalising the lenient spellings instead would be the wrong trade here, and a
// different one from the offset normalisation `Time` applies: RFC 3339 gives one
// instant many legal `full-time` spellings, but gives a `full-date` exactly one,
// so `2026-4-2` is not a conformant value written differently — it is not a
// conformant value.
impl FromStr for NgsiLdDate {
    type Err = NgsiLdDateError;

    fn from_str(value: &str) -> Result<NgsiLdDate, NgsiLdDateError> {
        let parsed = NaiveDate::parse_from_str(value, CANONICAL_FORMAT).map_err(|_| NgsiLdDateError::Malformed { value: value.to_owned() })?;
        let date = NgsiLdDate(parsed);
        if date.to_string() != value {
            return Err(NgsiLdDateError::Malformed { value: value.to_owned() });
        }
        Ok(date)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this adapter
// exists because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for NgsiLdDate {
    type Error = NgsiLdDateError;

    fn try_from(value: String) -> Result<NgsiLdDate, NgsiLdDateError> {
        value.parse()
    }
}

// `derive_more::Into` would yield `Into<NaiveDate>`, the inner type. What
// `#[serde(into = "String")]` needs is the canonical text, which no derive
// produces from a `chrono` value.
impl From<NgsiLdDate> for String {
    fn from(date: NgsiLdDate) -> String {
        date.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::ngsi_ld_date::{NgsiLdDate, NgsiLdDateError};

    #[test]
    fn a_canonical_date_round_trips_unchanged() {
        let parsed: NgsiLdDate = "2026-04-12".parse().expect("a full-date");
        pretty_assertions::assert_eq!(parsed.to_string(), "2026-04-12");
    }

    #[test]
    fn an_unpadded_spelling_is_refused_because_it_would_not_sort_chronologically() {
        let error = "2026-4-2".parse::<NgsiLdDate>().expect_err("the canonical form is zero-padded");
        assert!(matches!(error, NgsiLdDateError::Malformed { .. }), "got {error:?}");
    }

    #[test]
    fn text_that_is_not_a_calendar_date_is_refused() {
        for value in ["not-a-date", "2026-13-01", "2026-02-30", "2026-04-12T00:00:00Z"] {
            let error = value.parse::<NgsiLdDate>().expect_err("not a full-date");
            assert!(matches!(error, NgsiLdDateError::Malformed { .. }), "{value}: got {error:?}");
        }
    }

    #[test]
    fn byte_wise_order_over_the_canonical_form_is_chronological_order() {
        let ordered = ["1970-01-01", "1970-01-02", "1970-02-01", "1999-12-31", "2000-01-01", "2026-04-12"];
        let parsed: Vec<NgsiLdDate> = ordered.iter().map(|value| value.parse().expect("a full-date")).collect();
        for window in parsed.windows(2) {
            let (earlier, later) = (window[0], window[1]);
            assert!(earlier < later, "{earlier} must precede {later} chronologically");
            assert!(earlier.to_string() < later.to_string(), "{earlier} must precede {later} byte-wise as well");
        }
    }

    #[test]
    fn an_owned_string_parses_through_the_try_from_adapter() {
        let parsed = NgsiLdDate::try_from("2026-08-04".to_owned()).expect("an owned full-date");
        pretty_assertions::assert_eq!(String::from(parsed), "2026-08-04");
    }
}
