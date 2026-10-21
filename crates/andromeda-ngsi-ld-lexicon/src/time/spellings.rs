//! Every text clause 4.6.3 admits for one instant or one time of day.
//!
//! A value a client writes is held in the spelling it was written in, and the
//! clause admits several for one value: no fraction or "one or more fractional
//! digits, up to a maximum of six", written with the decimal point or, "in
//! requests", the comma. Every other component is fixed in width and the zone
//! is always `Z`, so the spellings of one value at the microsecond are finite,
//! at most thirteen, and a stored text names a value exactly when it is one of
//! them. A store that compares stored text can therefore compare a value
//! against all of them and keep the comparison an equality its indexes serve.

use crate::time::resolution::to_microsecond;
use chrono::{DateTime, NaiveTime, Timelike, Utc};

/// The fractional-second digits of a microsecond.
const MICROSECOND_DIGITS: usize = 6;

/// The two decimal separators clause 4.6.3 admits.
const SEPARATORS: [char; 2] = ['.', ','];

/// Every spelling of `instant` at the microsecond: `2024-01-01T00:00:00Z`,
/// `...:00.0Z` through `...:00.000000Z`, and each with a comma.
#[must_use]
pub fn instant_spellings(instant: DateTime<Utc>) -> Vec<String> {
    let instant = to_microsecond(instant);
    spellings(&instant.format("%Y-%m-%dT%H:%M:%S").to_string(), instant.nanosecond() / 1_000)
}

/// Every spelling of `time` at the microsecond: `12:00:00Z`, `12:00:00.0Z`
/// through `12:00:00.000000Z`, and each with a comma.
#[must_use]
pub fn time_of_day_spellings(time: NaiveTime) -> Vec<String> {
    let time = to_microsecond(time);
    spellings(&time.format("%H:%M:%S").to_string(), time.nanosecond() / 1_000)
}

/// The spellings of a value whose whole seconds read `skeleton` and whose
/// fraction is `micros` microseconds: without a fraction when it has none,
/// then every width from the digits it needs to six, with either separator.
fn spellings(skeleton: &str, micros: u32) -> Vec<String> {
    let digits = format!("{micros:0MICROSECOND_DIGITS$}");
    let needed = digits.trim_end_matches('0').len().max(1);
    let whole = (micros == 0).then(|| format!("{skeleton}Z"));
    let fractional = (needed..=MICROSECOND_DIGITS).flat_map(|width| {
        let fraction = &digits[..width];
        SEPARATORS.map(|separator| format!("{skeleton}{separator}{fraction}Z"))
    });
    whole.into_iter().chain(fractional).collect()
}

#[cfg(test)]
mod tests {
    use crate::time::{
        date_time::NgsiLdDateTime,
        rendering::{instant_text, time_of_day_text},
        spellings::{instant_spellings, time_of_day_spellings},
        time_of_day::NgsiLdTime,
    };
    use chrono::{DateTime, NaiveTime, TimeDelta, TimeZone, Utc};

    fn midnight() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("a valid instant")
    }

    #[test]
    fn a_whole_second_has_thirteen_spellings() {
        let spellings = instant_spellings(midnight());
        pretty_assertions::assert_eq!(spellings.len(), 13);
        assert!(spellings.contains(&"2024-01-01T00:00:00Z".to_owned()));
        assert!(spellings.contains(&"2024-01-01T00:00:00.000Z".to_owned()));
        assert!(spellings.contains(&"2024-01-01T00:00:00,000000Z".to_owned()));
    }

    #[test]
    fn a_fraction_is_spelled_from_the_digits_it_needs_to_six() {
        pretty_assertions::assert_eq!(
            instant_spellings(midnight() + TimeDelta::microseconds(123_450)),
            [
                "2024-01-01T00:00:00.12345Z",
                "2024-01-01T00:00:00,12345Z",
                "2024-01-01T00:00:00.123450Z",
                "2024-01-01T00:00:00,123450Z",
            ]
        );
    }

    /// Each spelling is one the reader admits and reads back as the same
    /// value, and the broker's own rendering is among them.
    #[test]
    fn every_instant_spelling_reads_back_as_the_instant() {
        for instant in [midnight(), midnight() + TimeDelta::milliseconds(250), midnight() + TimeDelta::microseconds(1)] {
            let spellings = instant_spellings(instant);
            assert!(spellings.contains(&instant_text(instant)), "{spellings:?}");
            for spelling in &spellings {
                pretty_assertions::assert_eq!(spelling.parse::<NgsiLdDateTime>().map(NgsiLdDateTime::get).ok(), Some(instant), "{spelling}");
            }
        }
    }

    #[test]
    fn every_time_of_day_spelling_reads_back_as_the_time_of_day() {
        for nano in [0, 500_000_000, 123_456_000] {
            let time = NaiveTime::from_hms_nano_opt(12, 0, 0, nano).expect("a time of day");
            let spellings = time_of_day_spellings(time);
            assert!(spellings.contains(&time_of_day_text(time)), "{spellings:?}");
            assert!(
                spellings.contains(&NgsiLdTime::normalise(time).to_string()),
                "the stored spelling: {spellings:?}"
            );
            for spelling in &spellings {
                pretty_assertions::assert_eq!(spelling.parse::<NgsiLdTime>().map(NgsiLdTime::get).ok(), Some(time), "{spelling}");
            }
        }
    }

    /// A value the broker produces itself can be finer than a microsecond,
    /// and is spelled at the microsecond it is held at.
    #[test]
    fn a_finer_instant_is_spelled_at_its_microsecond() {
        pretty_assertions::assert_eq!(
            instant_spellings(midnight() + TimeDelta::nanoseconds(1_999)).first().map(String::as_str),
            Some("2024-01-01T00:00:00.000001Z")
        );
    }
}
