//! The text the broker writes for an instant, a time of day or a date it holds.
//!
//! An instant has one spelling wherever the broker emits it: in a response, a
//! notification, a header, a forwarded request, a stored document or a
//! re-serialised `q`. It is UTC, ends in the literal `Z`, and carries exactly
//! the sub-second precision the instant has at the microsecond: none for a
//! whole second, three digits for a whole millisecond, six otherwise. Clause
//! 4.6.3 admits "one or more fractional digits, up to a maximum of six" and
//! fixes no single spelling, and the minimal one is what reproduces the
//! whole-second and millisecond spellings clients write, which the
//! conformance tests compare byte for byte.
//!
//! A time of day has the same minimal spelling where it is written back as
//! text a client reads, and a second, fixed-width spelling where the broker
//! stores it to order by: always six digits, so byte-wise order over the
//! stored text is chronological order.

use crate::time::resolution::to_microsecond;
use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Utc};

/// The spelling of an instant, at the microsecond: `2024-06-15T12:30:00Z`,
/// `...:00.750Z` or `...:00.123456Z`.
///
/// A finer instant (a clock reading carries nanoseconds) is spelled at the
/// microsecond it is stored at, so an answer written before storage and one
/// read back after agree.
#[must_use]
pub fn instant_text(instant: DateTime<Utc>) -> String {
    to_microsecond(instant).to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// The spelling of a time of day a client reads, at the microsecond:
/// `12:00:00Z`, `12:00:00.250Z` or `12:00:00.123456Z`.
#[must_use]
pub fn time_of_day_text(time: NaiveTime) -> String {
    to_microsecond(time).format("%H:%M:%S%.fZ").to_string()
}

/// The fixed-width spelling of a time of day the broker stores and orders by:
/// always six fractional digits, `12:00:00.000000Z`.
pub(crate) fn fixed_width_time_of_day_text(time: NaiveTime) -> String {
    to_microsecond(time).format("%H:%M:%S%.6fZ").to_string()
}

/// The spelling of a date, `2026-04-12`, which clause 4.6.3 fixes at one.
pub(crate) fn date_text(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use crate::time::rendering::{date_text, fixed_width_time_of_day_text, instant_text, time_of_day_text};
    use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, TimeZone, Utc};

    fn noon() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("a valid instant")
    }

    #[test]
    fn a_whole_second_is_spelled_without_a_fraction() {
        pretty_assertions::assert_eq!(instant_text(noon()), "2024-06-15T12:30:00Z");
    }

    #[test]
    fn a_whole_millisecond_is_spelled_with_three_digits() {
        pretty_assertions::assert_eq!(instant_text(noon() + TimeDelta::milliseconds(750)), "2024-06-15T12:30:00.750Z");
    }

    #[test]
    fn any_other_microsecond_is_spelled_with_six_digits() {
        pretty_assertions::assert_eq!(instant_text(noon() + TimeDelta::microseconds(123_456)), "2024-06-15T12:30:00.123456Z");
        pretty_assertions::assert_eq!(instant_text(noon() + TimeDelta::microseconds(90)), "2024-06-15T12:30:00.000090Z");
    }

    /// Clause 4.6.3 admits six fractional digits at most, so a nanosecond clock
    /// reading is spelled at the microsecond it is stored at.
    #[test]
    fn a_nanosecond_instant_is_spelled_at_its_microsecond() {
        pretty_assertions::assert_eq!(instant_text(noon() + TimeDelta::nanoseconds(90_386)), "2024-06-15T12:30:00.000090Z");
        pretty_assertions::assert_eq!(instant_text(noon() + TimeDelta::nanoseconds(123_000_999)), "2024-06-15T12:30:00.123Z");
    }

    #[test]
    fn a_time_of_day_a_client_reads_carries_its_minimal_fraction() {
        let time = |nano| NaiveTime::from_hms_nano_opt(12, 0, 0, nano).expect("a time of day");
        pretty_assertions::assert_eq!(time_of_day_text(time(0)), "12:00:00Z");
        pretty_assertions::assert_eq!(time_of_day_text(time(250_000_000)), "12:00:00.250Z");
        pretty_assertions::assert_eq!(time_of_day_text(time(123_456_789)), "12:00:00.123456Z");
    }

    #[test]
    fn a_stored_time_of_day_always_carries_six_digits() {
        let time = NaiveTime::from_hms_nano_opt(12, 0, 0, 500_000_999).expect("a time of day");
        pretty_assertions::assert_eq!(fixed_width_time_of_day_text(time), "12:00:00.500000Z");
    }

    #[test]
    fn a_date_is_zero_padded() {
        let date = NaiveDate::from_ymd_opt(26, 4, 2).expect("a date");
        pretty_assertions::assert_eq!(date_text(date), "0026-04-02");
    }
}
