//! An instant a few seconds from now, spelled the way a test writes an `expiresAt` it later compares as text: UTC with
//! three fraction digits.

use chrono::{Duration, Utc};

/// The instant `seconds` from now as `YYYY-MM-DDTHH:MM:SS.000Z`.
#[must_use]
pub fn seconds_ahead(seconds: i64) -> String {
    (Utc::now() + Duration::seconds(seconds)).format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

/// The instant `minutes` from now (negative for the past) as `YYYY-MM-DDTHH:MM:SSZ`, with no fraction.
#[must_use]
pub fn minutes_ahead(minutes: i64) -> String {
    (Utc::now() + Duration::minutes(minutes)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use crate::instant_ahead::{minutes_ahead, seconds_ahead};
    use googletest::prelude::*;

    #[gtest]
    fn the_instant_is_utc_with_three_zero_fraction_digits() {
        expect_that!(seconds_ahead(3), matches_regex(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.000Z$"));
    }

    #[gtest]
    fn a_whole_second_instant_has_no_fraction() {
        expect_that!(minutes_ahead(-15), matches_regex(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"));
    }
}
