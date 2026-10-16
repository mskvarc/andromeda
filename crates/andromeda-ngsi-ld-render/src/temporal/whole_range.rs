//! The one period an aggregation over the whole time range reports (clause
//! 4.5.19.1).
//!
//! An absent or zero `aggrPeriodDuration` "is interpreted as a duration
//! spanning the whole time range specified by the temporal query", one range
//! for the whole query, so every aggregated stream of a response reports the
//! same period. A side the query bounds is that bound. A side it leaves open
//! (the end of `after`, the start of `before`, both of an unbounded query)
//! names no instant of its own, and clause 4.5.19 makes each bound "the start
//! `DateTime`" or "the end `DateTime`" of the period, which `null` is not: it is
//! the extreme instant the response holds, the most recent for an open end and
//! the least recent for an open start. That is the instant clause 6.3.10 names
//! the `range-end` of a response by, taken over the response rather than per
//! Entity, so each stream reports its own extreme and the response closes them
//! all to the widest.

use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use chrono::{DateTime, Utc};

/// The start and end every whole-range period of one response reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WholeRange {
    /// The least recent start any stream reported.
    start: Option<DateTime<Utc>>,
    /// The most recent end any stream reported.
    end: Option<DateTime<Utc>>,
}

impl WholeRange {
    /// The whole range of a response whose whole-range periods are `rows`.
    ///
    /// A bounded side is the query's bound on every row, so its extreme is
    /// that bound; an open side is each stream's extreme instant, so its
    /// extreme is the response's.
    #[must_use]
    pub fn of_response<'a>(rows: impl IntoIterator<Item = &'a TemporalAggregationRow>) -> WholeRange {
        rows.into_iter().fold(WholeRange { start: None, end: None }, |range, row| WholeRange {
            start: [range.start, row.bucket_start].into_iter().flatten().min(),
            end: [range.end, row.bucket_end].into_iter().flatten().max(),
        })
    }

    /// Report the response's whole range on `row`.
    pub const fn close(&self, row: &mut TemporalAggregationRow) {
        row.bucket_start = self.start;
        row.bucket_end = self.end;
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::whole_range::WholeRange;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow};
    use chrono::{DateTime, Utc};

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid instant")
    }

    /// The whole-range period of `attribute`, from `start` to `end`.
    fn whole(attribute: &str, start: &str, end: &str) -> TemporalAggregationRow {
        TemporalAggregationRow {
            attribute_name: format!("https://example.com/{attribute}").parse().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: Some(at(start)),
            bucket_end: Some(at(end)),
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count: Some(1),
            distinct_count: None,
            group_keys: Vec::new(),
            stream_extent: StreamExtent::Whole,
        }
    }

    /// `timerel=after` from 2024-01-01: each stream ends at its own most
    /// recent instance, and the response closes them all at the latest of
    /// those, which is the `range-end` clause 6.3.10 names.
    #[test]
    fn an_open_whole_range_period_ends_at_the_latest_instant_of_the_response() {
        let mut rows = vec![
            whole("speed", "2024-01-01T00:00:00Z", "2024-01-02T15:00:00Z"),
            whole("fuelLevel", "2024-01-01T00:00:00Z", "2024-01-02T09:30:00Z"),
        ];
        let range = WholeRange::of_response(&rows);
        for row in &mut rows {
            range.close(row);
        }
        for row in &rows {
            pretty_assertions::assert_eq!(row.bucket_start, Some(at("2024-01-01T00:00:00Z")));
            pretty_assertions::assert_eq!(row.bucket_end, Some(at("2024-01-02T15:00:00Z")));
        }
    }

    /// `timerel=before` 2024-01-03: each stream starts at its own least
    /// recent instance, and the response opens them all at the earliest.
    #[test]
    fn an_open_whole_range_period_before_starts_at_the_earliest_instant() {
        let mut rows = vec![
            whole("speed", "2024-01-01T05:00:00Z", "2024-01-03T00:00:00Z"),
            whole("fuelLevel", "2024-01-01T02:00:00Z", "2024-01-03T00:00:00Z"),
        ];
        let range = WholeRange::of_response(&rows);
        for row in &mut rows {
            range.close(row);
        }
        for row in &rows {
            pretty_assertions::assert_eq!(row.bucket_start, Some(at("2024-01-01T02:00:00Z")));
            pretty_assertions::assert_eq!(row.bucket_end, Some(at("2024-01-03T00:00:00Z")));
        }
    }
}
