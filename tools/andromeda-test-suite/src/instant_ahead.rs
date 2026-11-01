//! An instant a few seconds from now, spelled the way a test writes an `expiresAt` it later compares as text: UTC with
//! three fraction digits.

use chrono::{Duration, Utc};

/// The instant `seconds` from now as `YYYY-MM-DDTHH:MM:SS.mmmZ`, keeping the millisecond of the clock it is read
/// from, so a window ending there is `seconds` long whatever the fraction of the current second.
#[must_use]
pub fn seconds_ahead(seconds: i64) -> String {
    (Utc::now() + Duration::seconds(seconds)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
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
    fn the_instant_is_utc_with_three_fraction_digits() {
        expect_that!(seconds_ahead(3), matches_regex(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$"));
    }

    #[gtest]
    fn a_whole_second_instant_has_no_fraction() {
        expect_that!(minutes_ahead(-15), matches_regex(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$"));
    }
}
