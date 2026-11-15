//! The clause 4.6.3 `DateTime` form as an anchored regular expression.
//!
//! A store that reads an instant out of text a client wrote, where the lexical
//! reader cannot run, matches the text against this form before its own input
//! function reads it, so it takes as an instant the texts [`NgsiLdDateTime`]
//! takes, the calendar check left to that input function: every component
//! present, the literal `Z`, a point or a comma before one to six fractional
//! digits, an hour below 24 and a minute and a second below 60. An input
//! function alone admits more: PostgreSQL's `timestamptz` takes an offset, a
//! bare date, `now` and a sixtieth second. The pattern uses only syntax that
//! POSIX extended regular expressions and the `regex` crate read alike.
//!
//! [`NgsiLdDateTime`]: crate::time::date_time::NgsiLdDateTime

/// The clause 4.6.3 `DateTime` form, anchored at both ends.
pub const DATE_TIME_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]([.,][0-9]{1,6})?Z$";

#[cfg(test)]
mod tests {
    use crate::time::{date_time::NgsiLdDateTime, lexical_pattern::DATE_TIME_PATTERN};
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
}
