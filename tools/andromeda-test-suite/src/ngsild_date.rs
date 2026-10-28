//! Dates as the ETSI suite reads them: the `NGSILD_DATE` form a date-valued member must take, and the instant an
//! `observedAt` stands for.
//!
//! The suite's `parse_ngsild_date` reads `YYYY-MM-DDTHH:MM:SS`, an optional fraction after `.` or `,`, and `Z`, keeping
//! six fraction digits; two `observedAt` values are the same instant when they agree on those.

use chrono::NaiveDateTime;
use regex::Regex;
use std::{iter, sync::LazyLock};

/// The dates the suite's `parse_ngsild_date` accepts, as the Hurl port's `NGSILD_DATE` pattern spells them: a valid
/// calendar date (leap years included), a time, and `Z` at the end or after a fraction.
static NGSILD_DATE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(([1-9][0-9]{3}|0[1-9][0-9]{2}|00[1-9][0-9]|000[1-9])-((0[13578]|1[02])-(0[1-9]|[12][0-9]|3[01])|",
        r"(0[469]|11)-(0[1-9]|[12][0-9]|30)|02-(0[1-9]|1[0-9]|2[0-8]))|([0-9]{2}(0[48]|[2468][048]|[13579][26])|",
        r"(0[48]|[2468][048]|[13579][26])00)-02-29)T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]([.,][0-9]+Z|Z$)",
    ))
});

/// The instant form `observedAt` takes: the seconds, an optional fraction after `.` or `,`, and `Z`.
static INSTANT: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:[.,](\d+))?Z$"));

/// Whether `text` is a date the suite reads.
#[must_use]
pub fn is_ngsild_date(text: &str) -> bool {
    NGSILD_DATE.as_ref().is_ok_and(|pattern| pattern.is_match(text))
}

/// The instant `text` stands for, to the microsecond, if it has the instant form.
#[must_use]
pub fn instant(text: &str) -> Option<(NaiveDateTime, u32)> {
    let captures = INSTANT.as_ref().ok()?.captures(text)?;
    let seconds = NaiveDateTime::parse_from_str(captures.get(1)?.as_str(), "%Y-%m-%dT%H:%M:%S").ok()?;
    let fraction = captures.get(2).map_or("", |fraction| fraction.as_str());
    let micros = fraction.chars().chain(iter::repeat('0')).take(6).collect::<String>().parse().ok()?;
    Some((seconds, micros))
}

/// Whether `left` and `right` are the same instant, as the suite compares two `observedAt` values.
#[must_use]
pub fn same_instant(left: &str, right: &str) -> bool {
    matches!((instant(left), instant(right)), (Some(left), Some(right)) if left == right)
}

#[cfg(test)]
mod tests {
    use crate::ngsild_date::{is_ngsild_date, same_instant};
    use googletest::prelude::*;

    #[gtest]
    fn a_utc_date_with_or_without_a_fraction_is_an_ngsild_date() {
        expect_that!(is_ngsild_date("2018-08-07T12:00:00Z"), eq(true));
        expect_that!(is_ngsild_date("2018-08-07T12:00:00.123456789Z"), eq(true));
        expect_that!(is_ngsild_date("2018-08-07T12:00:00,5Z"), eq(true));
        expect_that!(is_ngsild_date("2020-02-29T00:00:00Z"), eq(true));
    }

    #[gtest]
    fn an_offset_an_impossible_day_or_a_missing_zone_is_not() {
        expect_that!(is_ngsild_date("2018-08-07T12:00:00+01:00"), eq(false));
        expect_that!(is_ngsild_date("2019-02-29T00:00:00Z"), eq(false));
        expect_that!(is_ngsild_date("2018-08-07T12:00:00"), eq(false));
        expect_that!(is_ngsild_date("2018-08-07T24:00:00Z"), eq(false));
    }

    #[gtest]
    fn instants_agree_to_the_microsecond_whatever_their_spelling() {
        expect_that!(same_instant("2018-08-07T12:00:00Z", "2018-08-07T12:00:00.000Z"), eq(true));
        expect_that!(same_instant("2018-08-07T12:00:00,5Z", "2018-08-07T12:00:00.500000Z"), eq(true));
        expect_that!(same_instant("2018-08-07T12:00:00.1234567Z", "2018-08-07T12:00:00.123456Z"), eq(true));
        expect_that!(same_instant("2018-08-07T12:00:00Z", "2018-08-07T12:00:01Z"), eq(false));
        expect_that!(same_instant("2018-08-07T12:00:00Z", "not a date"), eq(false));
    }
}
