//! The one period boundary a partial aggregated temporal response is cut at
//! (clauses 4.5.19 and 6.3.10).
//!
//! An aggregated read pages every attribute stream by whole periods on its
//! own, and the store reports whether each stream holds periods beyond its
//! page. One response carries one `Content-Range`, which a client resumes the
//! whole response from, so every stream of every entity is cut at the same
//! instant, as an instance page is: forwards at the earliest end of the last
//! period any cut stream served, backwards at the latest start of the oldest
//! one. Every stream of a response shares one period grid, laid from one
//! origin, so that instant is a period boundary of every stream and the cut
//! never splits a period: a period is kept when it lies wholly on the kept
//! side.
//!
//! A forward page on a grid of whole months ends, where it can, on a boundary
//! no month clamped the day of: a client continuing from `range-end` lays its
//! periods from there, and only such a boundary lays the grid the response
//! was laid on (clause 4.5.19 places no period otherwise).

use crate::temporal::{history_row::TimeRange, page_cut::PageCut};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow};
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, period_grid::lays_the_grid};
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

    /// This cut moved to the nearest boundary of `rows` that lays the grid of
    /// whole months anchored at `anchor` (clause 4.5.19.1), or `None` when no
    /// such boundary lies within the rows on the kept side of the cut.
    ///
    /// A continuation opens on the boundary the page ends on and lays its grid
    /// from there, so only a boundary that [`lays_the_grid`] accepts lays the
    /// grid of the response. A forward cut moves back to the latest such end
    /// at or before it, and a backward cut moves forward to the earliest such
    /// start at or after it.
    #[must_use]
    pub fn on_whole_month_grid<'a>(self, rows: impl IntoIterator<Item = &'a TemporalAggregationRow>, anchor: DateTime<Utc>) -> Option<PageCut> {
        if lays_the_grid(anchor, self.boundary) {
            return Some(self);
        }
        let rows: Vec<&TemporalAggregationRow> = rows.into_iter().collect();
        let boundary = match self.direction {
            PageDirection::Forward => rows
                .iter()
                .filter_map(|row| row.bucket_end)
                .filter(|end| *end < self.boundary && lays_the_grid(anchor, *end))
                .max(),
            PageDirection::Backward { .. } => rows
                .iter()
                .filter_map(|row| row.bucket_start)
                .filter(|start| *start > self.boundary && lays_the_grid(anchor, *start))
                .min(),
        }?;
        Some(PageCut { boundary, ..self })
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

    /// The fewest instances the periods this cut keeps aggregate of any
    /// attribute stream `rows` hold, or `None` when they hold no stream or a
    /// kept period does not say how many instances it aggregates (or says a
    /// negative number).
    ///
    /// The groups of a grouped aggregation split one stream's periods, so
    /// their counts are summed into the stream; a stream the cut leaves no
    /// period of counts 0.
    #[must_use]
    pub fn fewest_kept_instances(&self, rows: &[TemporalAggregationRow]) -> Option<u64> {
        let mut streams: HashMap<StreamKey<'_>, u64> = HashMap::new();
        for row in rows {
            let kept = streams.entry((&row.attribute_name, row.dataset_id.as_ref())).or_default();
            if self.keeps_period(row) {
                *kept = kept.saturating_add(u64::try_from(row.total_count?).ok()?);
            }
        }
        streams.into_values().min()
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
    use andromeda_ngsi_ld_model::temporal::{GroupKeyValue, StreamExtent, TemporalAggregationRow};
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

    fn day(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single().expect("valid instant")
    }

    /// The monthly periods of a `P1M` grid laid from 31 July 2020, as `speed`
    /// served them, `count` of them.
    fn months_from_the_31st(count: usize) -> Vec<TemporalAggregationRow> {
        let boundaries = [
            day(2020, 7, 31),
            day(2020, 8, 31),
            day(2020, 9, 30),
            day(2020, 10, 31),
            day(2020, 11, 30),
            day(2020, 12, 31),
            day(2021, 1, 31),
            day(2021, 2, 28),
            day(2021, 3, 31),
        ];
        boundaries
            .windows(2)
            .take(count)
            .map(|bounds| TemporalAggregationRow {
                bucket_start: Some(bounds[0]),
                bucket_end: Some(bounds[1]),
                ..period("speed", 0, StreamExtent::Cut)
            })
            .collect()
    }

    /// The periods between consecutive `boundaries`, as `speed` served them.
    fn between(boundaries: &[DateTime<Utc>]) -> Vec<TemporalAggregationRow> {
        boundaries
            .windows(2)
            .map(|bounds| TemporalAggregationRow {
                bucket_start: Some(bounds[0]),
                bucket_end: Some(bounds[1]),
                ..period("speed", 0, StreamExtent::Cut)
            })
            .collect()
    }

    /// A month-end grid from 31 July keeps every boundary on a month end, so
    /// the page ends on 28 February, which lays the same grid.
    #[test]
    fn a_forward_cut_on_a_month_end_of_a_month_end_grid_stays() {
        let rows = months_from_the_31st(7);
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: day(2021, 2, 28),
        };
        pretty_assertions::assert_eq!(cut.on_whole_month_grid(&rows, day(2020, 7, 31)), Some(cut));
    }

    /// From 30 January a grid keeps the 30th, so the boundary of 28 February
    /// is clamped and lays no grid: the page ends on the latest end that does,
    /// 30 January.
    #[test]
    fn a_forward_cut_on_a_clamped_boundary_of_a_fixed_day_grid_moves_back_to_a_legal_end() {
        let rows = between(&[day(2020, 12, 30), day(2021, 1, 30), day(2021, 2, 28)]);
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: day(2021, 2, 28),
        };
        pretty_assertions::assert_eq!(cut.on_whole_month_grid(&rows, day(2020, 1, 30)).map(|cut| cut.boundary), Some(day(2021, 1, 30)));
    }

    /// 30 March keeps the 30th and is not a month end, so it lays the grid.
    #[test]
    fn a_forward_cut_on_a_fixed_day_boundary_that_lays_the_grid_stays() {
        let rows = between(&[day(2021, 1, 30), day(2021, 2, 28), day(2021, 3, 30)]);
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: day(2021, 3, 30),
        };
        pretty_assertions::assert_eq!(cut.on_whole_month_grid(&rows, day(2020, 1, 30)), Some(cut));
    }

    /// The rows hold no end on the 28th, so no end lays a grid from the 28th.
    #[test]
    fn a_forward_cut_with_no_legal_end_within_the_page_is_refused() {
        let rows = months_from_the_31st(7);
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: day(2021, 2, 28),
        };
        pretty_assertions::assert_eq!(cut.on_whole_month_grid(&rows, day(2020, 7, 28)), None);
    }

    /// A 29 February grid of years is a month-end grid, whose 28 February of a
    /// non-leap year lays the grid.
    #[test]
    fn a_yearly_cut_on_a_non_leap_month_end_of_a_month_end_grid_stays() {
        let rows = between(&[day(2024, 2, 29), day(2025, 2, 28), day(2026, 2, 28)]);
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: day(2025, 2, 28),
        };
        pretty_assertions::assert_eq!(cut.on_whole_month_grid(&rows, day(2024, 2, 29)), Some(cut));
    }

    #[test]
    fn a_backward_cut_on_a_boundary_that_lays_the_grid_stays() {
        let rows = months_from_the_31st(7);
        let cut = PageCut {
            direction: PageDirection::Backward {
                requested: NonZeroU32::new(30).expect("a positive count"),
            },
            boundary: day(2020, 9, 30),
        };
        pretty_assertions::assert_eq!(cut.on_whole_month_grid(&rows, day(2020, 7, 31)), Some(cut));
    }

    /// A page no stream of which was cut is complete.
    #[test]
    fn an_aggregated_page_no_stream_was_cut_in_has_no_cut() {
        let rows = stream("speed", 0, 39, StreamExtent::Whole);
        assert!(PageCut::of_periods(&rows, PageDirection::Forward).is_none());
    }

    fn counted(attribute: &str, offset: i64, total_count: Option<i64>) -> TemporalAggregationRow {
        TemporalAggregationRow {
            total_count,
            ..period(attribute, offset, StreamExtent::Cut)
        }
    }

    /// `speed` keeps two periods of 4 and 5 instances, `fuelLevel` two of 2
    /// each, its first one split over two groups; the period past the cut
    /// counts nothing.
    #[test]
    fn the_fewest_kept_instances_sums_the_kept_periods_of_each_stream() {
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: hour(2),
        };
        let mut grouped = counted("fuelLevel", 0, Some(1));
        grouped.group_keys = vec![GroupKeyValue::Value("a".to_owned())];
        let mut other_group = counted("fuelLevel", 0, Some(1));
        other_group.group_keys = vec![GroupKeyValue::Value("b".to_owned())];
        let rows = vec![
            counted("speed", 0, Some(4)),
            counted("speed", 1, Some(5)),
            counted("speed", 2, Some(50)),
            grouped,
            other_group,
            counted("fuelLevel", 1, Some(2)),
        ];
        pretty_assertions::assert_eq!(cut.fewest_kept_instances(&rows), Some(4));
    }

    #[test]
    fn a_kept_period_without_a_count_leaves_the_fewest_kept_instances_unknown() {
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: hour(2),
        };
        let rows = vec![counted("speed", 0, Some(4)), counted("fuelLevel", 0, None)];
        pretty_assertions::assert_eq!(cut.fewest_kept_instances(&rows), None);
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
