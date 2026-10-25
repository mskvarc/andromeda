//! Time-bucket modifiers for a temporal `aggrGroupBy` key (extension grammar §4.2).
//!
//! A `BucketTerm` is the `; bucketSpec` modifier a grouping term may carry after
//! a temporal key. It fills the gap clause 4.5.19 leaves: `aggrPeriodDuration` is
//! a fixed-width ISO-8601 duration, so it can express neither calendar-aligned
//! bucketing ("per calendar month", whose widths differ) nor cyclic bucketing
//! ("every reading in the 08:00-09:00 hour across all days"). A bucket is only
//! meaningful on a key whose value is a `DateTime`, `Date` or `Time`; the compile step
//! rejects it on any other key.

use strum::{Display, EnumString};

/// Whether a bucket's groups tile the timeline once or fold it into a cycle.
///
/// A calendar bucket is a contiguous truncation: each group is one interval of
/// the timeline. A cyclic bucket is non-contiguous: it collapses the timeline
/// onto a repeating field, so one group gathers instants from many periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketKind {
    /// A contiguous truncation to the start of the period (`date_trunc`).
    Calendar,
    /// A non-contiguous fold onto a repeating field (`extract`).
    Cyclic,
}

/// A time bucket applied to a temporal grouping key.
///
/// The camelCase spellings cross the API boundary as query text; the casing is
/// spec-facing rather than kebab-case to match the temporal member vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum BucketTerm {
    /// Calendar year (contiguous).
    Year,
    /// Calendar month (contiguous).
    Month,
    /// ISO week (contiguous).
    Week,
    /// Calendar day (contiguous).
    Day,
    /// Clock hour (contiguous).
    Hour,
    /// Day of the week, 0-6 (cyclic).
    DayOfWeek,
    /// Hour of the day, 0-23 (cyclic).
    HourOfDay,
    /// Month of the year, 1-12 (cyclic).
    MonthOfYear,
}

impl BucketTerm {
    /// Whether this bucket tiles the timeline (calendar) or folds it (cyclic).
    #[must_use]
    pub const fn kind(&self) -> BucketKind {
        match self {
            BucketTerm::Year | BucketTerm::Month | BucketTerm::Week | BucketTerm::Day | BucketTerm::Hour => BucketKind::Calendar,
            BucketTerm::DayOfWeek | BucketTerm::HourOfDay | BucketTerm::MonthOfYear => BucketKind::Cyclic,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::grouping::bucket_term::{BucketKind, BucketTerm};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn every_bucket_token_round_trips_through_its_camel_case_spelling() {
        for (bucket, text) in [
            (BucketTerm::Year, "year"),
            (BucketTerm::Month, "month"),
            (BucketTerm::Week, "week"),
            (BucketTerm::Day, "day"),
            (BucketTerm::Hour, "hour"),
            (BucketTerm::DayOfWeek, "dayOfWeek"),
            (BucketTerm::HourOfDay, "hourOfDay"),
            (BucketTerm::MonthOfYear, "monthOfYear"),
        ] {
            assert_eq!(bucket.to_string(), text, "{bucket:?}");
            assert_eq!(BucketTerm::from_str(text).expect("bucket parses"), bucket, "{text}");
        }
    }

    #[test]
    fn calendar_and_cyclic_buckets_classify_as_such() {
        for calendar in [BucketTerm::Year, BucketTerm::Month, BucketTerm::Week, BucketTerm::Day, BucketTerm::Hour] {
            assert_eq!(calendar.kind(), BucketKind::Calendar, "{calendar:?}");
        }
        for cyclic in [BucketTerm::DayOfWeek, BucketTerm::HourOfDay, BucketTerm::MonthOfYear] {
            assert_eq!(cyclic.kind(), BucketKind::Cyclic, "{cyclic:?}");
        }
    }

    #[test]
    fn an_unknown_bucket_token_is_rejected() {
        assert!(BucketTerm::from_str("bogus").is_err());
        assert!(BucketTerm::from_str("quarter").is_err());
    }
}
