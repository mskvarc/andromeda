//! The one period boundary a partial aggregated temporal response is cut at
//! (clauses 4.5.19 and 6.3.10).
//!
//! An aggregated read pages every attribute stream by whole periods on its
//! own, and the store reports whether each stream holds periods beyond its
//! page. One response carries one `Content-Range`, which a client resumes the
//! whole response from, so every stream of every entity is cut at the same
//! instant, as an instance page is: forwards at the earliest end of the last
//! period any cut stream served, backwards at the latest start of the oldest
//! one. Every stream of a response shares one period grid, anchored at the
//! query's `timeAt`, so that instant is a period boundary of every stream and
//! the cut never splits a period: a period is kept when it lies wholly on the
//! kept side.

use crate::temporal::{history_row::TimeRange, page_cut::PageCut};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow};
use andromeda_ngsi_ld_query::temporal::instance_page::PageDirection;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// One attribute stream: an attribute and the `datasetId` of its instances.
type StreamKey<'a> = (&'a AttributeName, Option<&'a Identifier>);

impl PageCut {
    /// The cut one entity's aggregated page implies, or `None` when the page
    /// left no period of any stream out.
    ///
    /// A stream the store reported periods beyond its page for was cut where
    /// its page ends: forwards at the end of its last period, backwards at the
    /// start of its first.
    #[must_use]
    pub fn of_periods(rows: &[TemporalAggregationRow], direction: PageDirection) -> Option<PageCut> {
        let mut streams: HashMap<StreamKey<'_>, DateTime<Utc>> = HashMap::new();
        for row in rows.iter().filter(|row| row.stream_extent == StreamExtent::Cut) {
            let edge = match direction {
                PageDirection::Forward => row.bucket_end,
                PageDirection::Backward { .. } => row.bucket_start,
            };
            let Some(edge) = edge else {
                continue;
            };
            streams
                .entry((&row.attribute_name, row.dataset_id.as_ref()))
                .and_modify(|held| {
                    *held = match direction {
                        PageDirection::Forward => (*held).max(edge),
                        PageDirection::Backward { .. } => (*held).min(edge),
                    };
                })
                .or_insert(edge);
        }
        let edges = streams.into_values();
        let boundary = match direction {
            PageDirection::Forward => edges.min(),
            PageDirection::Backward { .. } => edges.max(),
        }?;
        Some(PageCut { direction, boundary })
    }

    /// Whether an aggregated period stays in the response under this cut:
    /// forwards when it ends at or before the boundary, backwards when it
    /// starts at or after it.
    #[must_use]
    pub fn keeps_period(&self, row: &TemporalAggregationRow) -> bool {
        match self.direction {
            PageDirection::Forward => row.bucket_end.is_some_and(|end| end <= self.boundary),
            PageDirection::Backward { .. } => row.bucket_start.is_some_and(|start| start >= self.boundary),
        }
    }
}

/// The least recent start and the most recent end of any period `rows` hold,
/// which clause 6.3.10 reports an aggregated page by: forwards its `range-end`
/// is the end of the last period served, backwards the start of the oldest.
#[must_use]
pub fn period_time_range<'a>(rows: impl IntoIterator<Item = &'a TemporalAggregationRow>) -> Option<TimeRange> {
    rows.into_iter()
        .filter_map(|row| row.bucket_start.zip(row.bucket_end))
        .reduce(|(start, end), (row_start, row_end)| (start.min(row_start), end.max(row_end)))
}

#[cfg(test)]
mod tests {
    use crate::temporal::{page_cut::PageCut, period_cut::period_time_range};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow};
    use andromeda_ngsi_ld_query::temporal::instance_page::PageDirection;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use std::num::NonZeroU32;

    fn hour(offset: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant") + Duration::hours(offset)
    }

    /// The hourly period starting at `hour(offset)` of `attribute`, in a
    /// stream the page read to `extent`.
    fn period(attribute: &str, offset: i64, extent: StreamExtent) -> TemporalAggregationRow {
        TemporalAggregationRow {
            attribute_name: format!("https://example.com/{attribute}").parse().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: Some(hour(offset)),
            bucket_end: Some(hour(offset + 1)),
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count: Some(1),
            distinct_count: None,
            group_keys: Vec::new(),
            stream_extent: extent,
        }
    }

    /// The hourly periods `from..=to` of a stream read to `extent`.
    fn stream(attribute: &str, from: i64, to: i64, extent: StreamExtent) -> Vec<TemporalAggregationRow> {
        (from..=to).map(|offset| period(attribute, offset, extent)).collect()
    }

    /// `speed` served 00:00-19:00 and `fuelLevel` 03:00-23:00, both cut; the
    /// response ends where `speed`'s last period ends, and `fuelLevel` keeps
    /// only the periods wholly before it. A stream that was not cut takes no
    /// part in the boundary.
    #[test]
    fn an_aggregated_page_is_cut_at_the_earliest_period_boundary_any_stream_was_cut_at() {
        let mut rows = stream("speed", 0, 18, StreamExtent::Cut);
        rows.extend(stream("fuelLevel", 3, 22, StreamExtent::Cut));
        rows.extend(stream("heading", 0, 30, StreamExtent::Whole));
        let cut = PageCut::of_periods(&rows, PageDirection::Forward).expect("two streams were cut");
        pretty_assertions::assert_eq!(cut.boundary, hour(19));
        let kept: Vec<&TemporalAggregationRow> = rows.iter().filter(|row| cut.keeps_period(row)).collect();
        assert!(kept.iter().all(|row| row.bucket_end <= Some(hour(19))));
        pretty_assertions::assert_eq!(kept.len(), 19 + 16 + 19);
    }

    /// Backwards the response keeps the periods at or after the latest start
    /// any cut stream's oldest period has.
    #[test]
    fn a_backward_aggregated_page_is_cut_at_the_latest_period_start_any_stream_was_cut_at() {
        let mut rows = stream("speed", 21, 39, StreamExtent::Cut);
        rows.extend(stream("fuelLevel", 25, 39, StreamExtent::Cut));
        let direction = PageDirection::Backward {
            requested: NonZeroU32::new(30).expect("a positive count"),
        };
        let cut = PageCut::of_periods(&rows, direction).expect("both streams were cut");
        pretty_assertions::assert_eq!(cut.boundary, hour(25));
        assert!(rows.iter().filter(|row| cut.keeps_period(row)).all(|row| row.bucket_start >= Some(hour(25))));
    }

    /// A page no stream of which was cut is complete.
    #[test]
    fn an_aggregated_page_no_stream_was_cut_in_has_no_cut() {
        let rows = stream("speed", 0, 39, StreamExtent::Whole);
        assert!(PageCut::of_periods(&rows, PageDirection::Forward).is_none());
    }

    /// Clause 6.3.10: forwards the `range-end` is the most recent timestamp in
    /// the range of the response, which for whole periods is the end of the
    /// last one served, not the start of it nor the last instance inside it.
    #[test]
    fn the_content_range_of_an_aggregated_page_ends_at_its_last_period_end() {
        let rows = stream("speed", 0, 18, StreamExtent::Cut);
        let cut = PageCut::of_periods(&rows, PageDirection::Forward).expect("the stream was cut");
        let kept = rows.iter().filter(|row| cut.keeps_period(row));
        pretty_assertions::assert_eq!(period_time_range(kept), Some((hour(0), hour(19))));
    }
}
