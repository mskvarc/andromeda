//! The one cut a partial temporal response is made at, and the continuation
//! it implies (clause 6.3.10).
//!
//! The cut fixes where the response's `Content-Range` ends. The continuation
//! also needs what the cut left each attribute stream: a backward page asks
//! the next one for the rest of the client's `lastN`, which is counted from
//! the sparsest stream. That count is taken over the pages before the cut
//! drops anything, since a stream the cut emptied leaves the page entirely.

use crate::{
    read::{aggregated_page::EntityAggregationPage, rendered_entity::EntityHistoryPage},
    service::TemporalRange,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::{
    continuation::{PageEdge, TemporalContinuation},
    window::TimeInterval,
};
use andromeda_ngsi_ld_render::temporal::{history_row::TimeRange, page_cut::PageCut};

/// The cut every entity of a partial response is made at, with what it
/// served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResponseCut {
    /// The boundary every stream of every entity is cut at.
    pub(crate) cut: PageCut,
    /// What the response ends on: an instant for an instance page, a period
    /// boundary for an aggregated one.
    edge: PageEdge,
    /// The fewest instances the cut kept of any attribute stream of the
    /// response; `None` when an aggregated period did not count its instances.
    fewest_served: Option<u64>,
}

impl ResponseCut {
    /// The cut `cut` makes of the instance pages `pages`.
    pub(crate) fn of_instances(cut: PageCut, pages: &[EntityHistoryPage], timeproperty: TimeProp) -> ResponseCut {
        let fewest = pages.iter().filter_map(|page| cut.fewest_kept(&page.rows, timeproperty)).min();
        ResponseCut {
            cut,
            edge: PageEdge::Instant(cut.boundary),
            fewest_served: fewest.map(|fewest| u64::try_from(fewest).unwrap_or(u64::MAX)),
        }
    }

    /// The cut `cut` makes of the aggregated pages `pages`, read before the
    /// cut drops the periods past it.
    pub(crate) fn of_periods(cut: PageCut, pages: &[EntityAggregationPage]) -> ResponseCut {
        let counts: Option<Vec<u64>> = pages
            .iter()
            .filter(|page| !page.rows.is_empty())
            .map(|page| cut.fewest_kept_instances(&page.rows))
            .collect();
        ResponseCut {
            cut,
            edge: PageEdge::PeriodBoundary(cut.boundary),
            fewest_served: counts.and_then(|counts| counts.into_iter().min()),
        }
    }

    /// The range a response of `interval` carrying `observed` reports under
    /// this cut, with the request that continues it.
    pub(crate) fn range(self, interval: TimeInterval, observed: Option<TimeRange>) -> TemporalRange {
        TemporalRange {
            interval,
            observed,
            direction: self.cut.direction,
            next: TemporalContinuation::following(interval, self.cut.direction, self.edge, self.fewest_served),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::read::{
        aggregated_page::EntityAggregationPage,
        rendered_entity::{EntityHistoryPage, HistoryPresence},
        response_cut::ResponseCut,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::temporal::{continuation::TemporalContinuation, instance_page::PageDirection, window::TimeInterval};
    use andromeda_ngsi_ld_render::temporal::page_cut::PageCut;
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;
    use uuid::Uuid;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid instant")
    }

    fn minute(offset: i64) -> DateTime<Utc> {
        at("2020-01-01T01:00:00Z") + Duration::minutes(offset)
    }

    fn row(attribute: &str, offset: i64) -> TemporalHistoryRow {
        TemporalHistoryRow {
            attribute_name: format!("https://example.com/{attribute}").parse().expect("a valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number {
                number: JsonNumber::from(offset),
                datatype: None,
            },
            observed_at: Some(minute(offset)),
            recorded_at: minute(offset),
            created_at: None,
            modified_at: None,
            instance: EvolutionInstance::Attribute(InstanceIdentity::Generated(Uuid::new_v4())),
            metadata: None,
            stream_extent: StreamExtent::Cut,
        }
    }

    fn history_page(entity: &str, rows: Vec<TemporalHistoryRow>) -> EntityHistoryPage {
        EntityHistoryPage {
            entity_id: entity.parse().expect("a valid identifier"),
            rows,
        }
    }

    fn backward_cut(boundary: DateTime<Utc>) -> PageCut {
        PageCut {
            direction: PageDirection::Backward {
                requested: NonZeroU32::new(50).expect("a positive lastN"),
            },
            boundary,
        }
    }

    /// Vehicle 1's `speed` kept 10 instances, vehicle 2's `fuelLevel` 4: the
    /// continuation asks for the 46 the sparsest stream is still owed.
    #[test]
    fn a_backward_instance_cut_continues_with_the_last_n_its_sparsest_stream_is_owed() {
        let pages = [
            history_page("urn:ngsi-ld:Vehicle:1", (10..20).map(|offset| row("speed", offset)).collect()),
            history_page("urn:ngsi-ld:Vehicle:2", (16..20).map(|offset| row("fuelLevel", offset)).collect()),
        ];
        let range = ResponseCut::of_instances(backward_cut(minute(10)), &pages, TimeProp::Observed).range(TimeInterval::Unbounded, None);
        assert_eq!(
            range.next,
            Some(TemporalContinuation {
                interval: TimeInterval::Before(minute(10)),
                last_n: NonZeroU32::new(46),
            })
        );
    }

    fn period(attribute: &str, hour: i64, total_count: Option<i64>) -> TemporalAggregationRow {
        let start = at("2020-01-01T00:00:00Z") + Duration::hours(hour);
        TemporalAggregationRow {
            attribute_name: format!("https://example.com/{attribute}").parse().expect("a valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: Some(start),
            bucket_end: Some(start + Duration::hours(1)),
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count,
            distinct_count: None,
            group_keys: Vec::new(),
            stream_extent: StreamExtent::Cut,
        }
    }

    fn aggregation_page(entity: &str, rows: Vec<TemporalAggregationRow>) -> EntityAggregationPage {
        EntityAggregationPage::read(entity.parse().expect("a valid identifier"), rows)
    }

    /// The periods from 10:00 on are kept: `speed` aggregates 3 + 4
    /// instances, `fuelLevel` 2, its 08:00 period lying past the cut.
    #[test]
    fn a_backward_period_cut_continues_with_the_instances_its_sparsest_stream_is_owed() {
        let pages = [aggregation_page(
            "urn:ngsi-ld:Vehicle:1",
            vec![
                period("speed", 10, Some(3)),
                period("speed", 11, Some(4)),
                period("fuelLevel", 8, Some(9)),
                period("fuelLevel", 11, Some(2)),
            ],
        )];
        assert!(matches!(pages[0].presence(), HistoryPresence::Present));
        let boundary = at("2020-01-01T10:00:00Z");
        let range = ResponseCut::of_periods(backward_cut(boundary), &pages).range(TimeInterval::Unbounded, None);
        assert_eq!(
            range.next,
            Some(TemporalContinuation {
                interval: TimeInterval::Before(boundary),
                last_n: NonZeroU32::new(48),
            })
        );
    }

    /// A period that does not say what it aggregates leaves the rest of the
    /// `lastN` unknown, so no continuation is offered.
    #[test]
    fn a_backward_period_cut_over_an_uncounted_period_has_no_continuation() {
        let pages = [aggregation_page("urn:ngsi-ld:Vehicle:1", vec![period("speed", 10, None)])];
        let range = ResponseCut::of_periods(backward_cut(at("2020-01-01T10:00:00Z")), &pages).range(TimeInterval::Unbounded, None);
        assert_eq!(range.next, None);
    }

    /// A forward aggregated cut opens the next request on its boundary.
    #[test]
    fn a_forward_period_cut_continues_on_its_boundary() {
        let pages = [aggregation_page("urn:ngsi-ld:Vehicle:1", vec![period("speed", 0, Some(1))])];
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: at("2020-01-01T01:00:00Z"),
        };
        let range = ResponseCut::of_periods(cut, &pages).range(TimeInterval::After(at("2020-01-01T00:00:00Z")), None);
        assert_eq!(range.next.map(|next| next.interval), Some(TimeInterval::After(at("2020-01-01T01:00:00Z"))));
    }
}
