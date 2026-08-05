//! Comparison operands of a `q` predicate (clause 4.9).

use crate::error::QueryError;
use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Utc};
use serde_json::Number;

/// The right-hand side of a comparison predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareValue {
    /// A single scalar value (e.g. `speed>50`).
    Scalar(Scalar),
    /// A range of two values (e.g. `speed==10..20`).
    Range {
        /// The lower bound of the range (inclusive).
        min: Scalar,
        /// The upper bound of the range (inclusive).
        max: Scalar,
    },
    /// A list of values (e.g. `status==\"active\",\"pending\"`).
    ValueList(Vec<Scalar>),
}

/// A scalar value in a `q` expression.
///
/// The three temporal variants carry parsed values rather than the client's
/// text. Clause 4.6.3 defines them as IETF RFC 3339 `date-time`, `full-date`
/// and `full-time`, all of which pin an instant (or an instant within a day)
/// exactly, so an operand that does not parse is a malformed request and is
/// rejected where the filter is parsed. Offsets are normalised to UTC, which is
/// what makes `12:00:00+01:00` and `11:00:00Z` the same operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    /// A numeric value (integer or floating-point).
    Number(Number),
    /// A boolean value (`true` or `false`).
    Bool(bool),
    /// A quoted string value.
    Str(String),
    /// An RFC 3339 `date-time`, normalised to UTC.
    DateTime(DateTime<Utc>),
    /// An RFC 3339 `full-date`.
    Date(NaiveDate),
    /// An RFC 3339 `full-time`, normalised to UTC.
    Time(NaiveTime),
}

/// Parse an RFC 3339 `date-time` operand, normalising it to UTC.
///
/// Returns the instant rather than a [`Scalar`] wrapping it: a caller that
/// needs the value back out of the variant it was just placed in has to prove
/// the variant, and there is nothing to prove here.
pub fn parse_date_time(raw: &str) -> Result<DateTime<Utc>, QueryError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|source| QueryError::QInvalidDateTime {
            value: raw.to_string(),
            source,
        })
}

/// Parse an RFC 3339 `full-date` operand.
pub fn parse_date(raw: &str) -> Result<NaiveDate, QueryError> {
    raw.parse::<NaiveDate>().map_err(|source| QueryError::QInvalidDate {
        value: raw.to_string(),
        source,
    })
}

/// Parse an RFC 3339 `full-time` operand, normalising it to UTC.
///
/// RFC 3339 requires a `full-time` to carry an offset, so the operand is
/// resolved against an arbitrary date and the date discarded: only the offset
/// matters, and it shifts every date identically.
pub fn parse_time(raw: &str) -> Result<NaiveTime, QueryError> {
    let anchored = format!("{TIME_ANCHOR_DATE}T{raw}");
    DateTime::parse_from_rfc3339(&anchored)
        .map(|parsed| parsed.with_timezone(&Utc).time())
        .map_err(|source| QueryError::QInvalidTime {
            value: raw.to_string(),
            source,
        })
}

impl Scalar {
    /// The operand's canonical text form.
    ///
    /// One rendering serves both the clause 4.9 serialisation and every SQL
    /// binding, so a filter that round-trips through the query string and a
    /// filter that reaches PostgreSQL carry the same literal by construction.
    /// Temporal operands render in RFC 3339 UTC form.
    pub fn literal_text(&self) -> String {
        match self {
            Scalar::Number(number) => number.to_string(),
            Scalar::Bool(value) => value.to_string(),
            // clone: callers own the rendered literal
            Scalar::Str(value) => value.clone(),
            Scalar::DateTime(value) => value.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            Scalar::Date(value) => value.to_string(),
            Scalar::Time(value) => value.format(TIME_SQL_FORMAT).to_string(),
        }
    }

    /// The operand's text form and the cast the comparison needs around it.
    ///
    /// Used by every SQL rendering that compares against JSONB, where the value
    /// arrives as text from `#>>` and both sides have to be cast to the same
    /// type before they are ordered. Without the cast a numeric comparison
    /// would order lexicographically, so `9` would exceed `10`.
    ///
    /// Temporal operands cast to nothing: PostgreSQL orders canonical RFC 3339
    /// UTC text identically to the instants it denotes, because a fixed-width,
    /// fixed-offset representation sorts the same way lexicographically and
    /// chronologically.
    pub fn as_sql_text(&self) -> (&'static str, String) {
        let cast = match self {
            Scalar::Number(_) => "::numeric",
            Scalar::Bool(_) => "::boolean",
            Scalar::Str(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => "",
        };
        (cast, self.literal_text())
    }
}

/// The date a bare `full-time` operand is anchored to while its offset is
/// resolved. Discarded immediately afterwards; any date would do.
const TIME_ANCHOR_DATE: &str = "1970-01-01";

/// Canonical rendering of a UTC time of day: seconds always, fractional
/// seconds only when they are non-zero, and an explicit `Z`.
const TIME_SQL_FORMAT: &str = "%H:%M:%S%.fZ";

#[cfg(test)]
mod tests {
    use super::*;

    fn date_time(raw: &str) -> Scalar {
        Scalar::DateTime(parse_date_time(raw).expect("valid date-time"))
    }

    #[test]
    fn a_number_carries_a_numeric_cast_so_it_is_not_ordered_as_text() {
        assert_eq!(Scalar::Number(9.into()).as_sql_text(), ("::numeric", "9".to_string()));
    }

    #[test]
    fn a_boolean_carries_a_boolean_cast() {
        assert_eq!(Scalar::Bool(true).as_sql_text(), ("::boolean", "true".to_string()));
    }

    #[test]
    fn text_and_temporal_operands_need_no_cast() {
        assert_eq!(Scalar::Str("red".to_string()).as_sql_text(), ("", "red".to_string()));
        assert_eq!(date_time("2024-01-01T00:00:00Z").as_sql_text(), ("", "2024-01-01T00:00:00Z".to_string()));
        assert_eq!(Scalar::Date(parse_date("2024-01-01").unwrap()).as_sql_text(), ("", "2024-01-01".to_string()));
        assert_eq!(Scalar::Time(parse_time("12:00:00Z").unwrap()).as_sql_text(), ("", "12:00:00Z".to_string()));
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a date-time operand is an RFC 3339 date-time, so it is
        // resolved to an instant here rather than re-read on every rendering.
        let parsed = parse_date_time("2017-12-24T12:00:00Z").expect("valid date-time");
        assert_eq!(parsed.to_rfc3339_opts(SecondsFormat::Secs, true), "2017-12-24T12:00:00Z");
    }

    #[test]
    fn an_unparseable_date_time_operand_is_rejected() {
        let error = parse_date_time("2017-13-45").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDateTime { .. }), "got {error:?}");
    }

    #[test]
    fn an_unparseable_date_operand_is_rejected() {
        let error = parse_date("2017-13-45").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDate { .. }), "got {error:?}");
    }

    #[test]
    fn an_unparseable_time_operand_is_rejected() {
        let error = parse_time("25:99:99Z").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidTime { .. }), "got {error:?}");
    }

    #[test]
    fn an_offset_bearing_operand_normalises_to_utc() {
        // RFC 3339 fixes the instant, so the same moment written in two zones
        // is one operand.
        assert_eq!(date_time("2017-12-24T13:00:00+01:00"), date_time("2017-12-24T12:00:00Z"));
        assert_eq!(parse_time("13:00:00+01:00").unwrap(), parse_time("12:00:00Z").unwrap());
    }

    #[test]
    fn fractional_seconds_survive_the_canonical_rendering() {
        assert_eq!(
            date_time("2024-01-01T00:00:00.250Z").as_sql_text(),
            ("", "2024-01-01T00:00:00.250Z".to_string())
        );
        assert_eq!(
            Scalar::Time(parse_time("12:00:00.250Z").unwrap()).as_sql_text(),
            ("", "12:00:00.250Z".to_string())
        );
    }
}
