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
//!
//! The response closes in two stages: the rows of one read within it, and the
//! entities across the reads that make up one response (the entities read
//! locally and the ones rendered over merged instances).

use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, aggregated_entity::AggregatedTemporalEntity};
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

    /// The whole range of a response whose whole-range periods are those the
    /// `entities` hold; an entity holding no period adds nothing.
    #[must_use]
    pub fn of_entities<'a>(entities: impl IntoIterator<Item = &'a AggregatedTemporalEntity>) -> WholeRange {
        entities
            .into_iter()
            .filter_map(AggregatedTemporalEntity::period_span)
            .fold(WholeRange { start: None, end: None }, |range, (start, end)| WholeRange {
                start: [range.start, Some(start)].into_iter().flatten().min(),
                end: [range.end, Some(end)].into_iter().flatten().max(),
            })
    }

    /// Report the response's whole range on `row`.
    pub const fn close(&self, row: &mut TemporalAggregationRow) {
        row.bucket_start = self.start;
        row.bucket_end = self.end;
    }

    /// Report the response's whole range on every period of `entity`. An
    /// entity holding no period stays as it is.
    pub fn close_entity(&self, entity: &mut AggregatedTemporalEntity) {
        if let Some((start, end)) = self.start.zip(self.end) {
            entity.close_periods_to(start, end);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::whole_range::WholeRange;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_model::{
        entity_type::EntityTypeValue,
        temporal::{StreamExtent, TemporalAggregationRow, aggregated_entity::AggregatedTemporalEntity},
    };
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

    /// The entity the `rows` make of `urn:ngsi-ld:Vehicle:1`, with no grouping.
    fn entity_of(rows: &[TemporalAggregationRow]) -> AggregatedTemporalEntity {
        AggregatedTemporalEntity::from_rows(
            "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
            EntityTypeValue::Single("Vehicle".parse().expect("a type")),
            rows,
            &[AggrMethod::TotalCount],
            0,
            AttributeName::clone,
        )
    }

    /// Two entities of one response, one ending at 10:00 and one at 11:00,
    /// close to the later end and the least recent start.
    #[test]
    fn entities_close_to_the_widest_span_they_hold() {
        let mut early = entity_of(&[whole("speed", "2024-01-01T00:00:00Z", "2024-01-01T10:00:00Z")]);
        let mut late = entity_of(&[whole("speed", "2024-01-01T01:00:00Z", "2024-01-01T11:00:00Z")]);
        let empty = entity_of(&[]);
        let range = WholeRange::of_entities([&early, &late, &empty]);
        range.close_entity(&mut early);
        range.close_entity(&mut late);
        let span = Some((at("2024-01-01T00:00:00Z"), at("2024-01-01T11:00:00Z")));
        pretty_assertions::assert_eq!(early.period_span(), span);
        pretty_assertions::assert_eq!(late.period_span(), span);
    }

    /// An entity holding no period names no range and is left as it is.
    #[test]
    fn an_entity_without_a_period_is_skipped() {
        let mut empty = entity_of(&[]);
        let range = WholeRange::of_entities([&empty]);
        range.close_entity(&mut empty);
        pretty_assertions::assert_eq!(empty.period_span(), None);
    }
}
