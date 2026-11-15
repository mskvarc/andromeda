//! The clause 4.6.3 `DateTime`, `Date` and `Time` forms as anchored regular
//! expressions.
//!
//! A store that reads a time value out of text a client wrote, where the
//! lexical reader cannot run, matches the text against the form before its own
//! input function reads it, so it takes as a value of the form the texts the
//! lexical reader takes: every component present, the literal `Z`, a point or
//! a comma before one to six fractional digits, an hour below 24 and a minute
//! and a second below 60. The calendar check is left to the store: a pattern
//! takes `2024-02-30`, which no reader takes. An input function alone admits
//! more: PostgreSQL's `timestamptz` takes an offset, a bare date, `now` and a
//! sixtieth second, and its `date` input an unpadded month. Each pattern uses
//! only syntax that POSIX extended regular expressions and the `regex` crate
//! read alike.

/// The clause 4.6.3 `DateTime` form, anchored at both ends.
pub const DATE_TIME_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]([.,][0-9]{1,6})?Z$";

/// The clause 4.6.3 `Date` form, anchored at both ends.
pub const DATE_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";

/// The clause 4.6.3 `Time` form, anchored at both ends: the clock range is in
/// the pattern, so a text it matches is a time of day.
pub const TIME_PATTERN: &str = "^([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]([.,][0-9]{1,6})?Z$";

#[cfg(test)]
mod tests {
    use crate::time::{
        date::NgsiLdDate,
        date_time::NgsiLdDateTime,
        lexical_pattern::{DATE_PATTERN, DATE_TIME_PATTERN, TIME_PATTERN},
        time_of_day::NgsiLdTime,
    };
    use regex::Regex;

    /// The pattern matches exactly the texts the lexical reader takes as a
    /// `DateTime`, over every form a text names a calendar instant in.
    #[test]
    fn the_pattern_matches_exactly_what_the_lexical_reader_takes() {
        let pattern = Regex::new(DATE_TIME_PATTERN).expect("a valid pattern");
        for text in [
            "2024-01-01T00:00:00Z",
            "2024-01-01T23:59:59.5Z",
            "2024-01-01T00:00:00,123456Z",
            "2024-01-01T00:00:00.1234567Z",
            "2024-01-01T00:00:00+00:00",
            "2024-01-01T00:00:00",
            "2024-01-01",
            "2024-01-01T00:00Z",
            "2024-01-01t00:00:00z",
            "2024-01-01T00:00:60Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:60:00Z",
            "2024-01-01T00:00:00.Z",
            " 2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z ",
            "now",
        ] {
            pretty_assertions::assert_eq!(pattern.is_match(text), text.parse::<NgsiLdDateTime>().is_ok(), "{text}");
        }
    }

    /// The `Date` pattern matches exactly the texts the lexical reader takes
    /// as a `Date`, over spellings of real calendar days.
    #[test]
    fn the_date_pattern_matches_exactly_what_the_lexical_reader_takes() {
        let pattern = Regex::new(DATE_PATTERN).expect("a valid pattern");
        for text in [
            "2024-01-01",
            "2024-12-31",
            "2024-1-1",
            "24-01-01",
            "20240101",
            "2024/01/01",
            "2024-01-01Z",
            "2024-01-01T00:00:00Z",
            " 2024-01-01",
            "2024-01-01 ",
            "today",
        ] {
            pretty_assertions::assert_eq!(pattern.is_match(text), text.parse::<NgsiLdDate>().is_ok(), "{text}");
        }
    }

    /// The `Date` pattern leaves the calendar to the store: it takes a day the
    /// calendar does not have, which the lexical reader refuses.
    #[test]
    fn the_date_pattern_leaves_the_calendar_check_to_the_store() {
        let pattern = Regex::new(DATE_PATTERN).expect("a valid pattern");
        assert!(pattern.is_match("2024-02-30"));
        assert!("2024-02-30".parse::<NgsiLdDate>().is_err());
    }

    /// The `Time` pattern matches exactly the texts the lexical reader takes
    /// as a `Time`, the clock range included.
    #[test]
    fn the_time_pattern_matches_exactly_what_the_lexical_reader_takes() {
        let pattern = Regex::new(TIME_PATTERN).expect("a valid pattern");
        for text in [
            "08:00:00Z",
            "23:59:59.5Z",
            "00:00:00,123456Z",
            "00:00:00.1234567Z",
            "08:00:00+01:00",
            "08:00:00",
            "08:00Z",
            "8:00:00Z",
            "08:00:00z",
            "24:00:00Z",
            "12:60:00Z",
            "12:00:60Z",
            "12:00:00.Z",
            "T08:00:00Z",
            " 08:00:00Z",
            "08:00:00Z ",
            "noon",
        ] {
            pretty_assertions::assert_eq!(pattern.is_match(text), text.parse::<NgsiLdTime>().is_ok(), "{text}");
        }
    }
}
