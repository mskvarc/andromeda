//! How far the merged history of a federated temporal query is complete
//! (clause 6.3.10).
//!
//! The local read and every context source each answer with as much of an
//! entity's history as they can at once, and one that stops short says where:
//! the local read by the cut it made, a source by the `Content-Range` of its
//! 206. Clause 6.3.10 gives the response a single `Content-Range` whose
//! range-end is "the most recent timestamp in the range of the response" when
//! paging forwards and "the least recent timestamp" when paging backwards with
//! `lastN`, and the client resumes from it. So the merged page is complete only
//! up to the earliest of those stops forwards, and down to the latest of them
//! backwards; instances past that bound are left out rather than answered
//! under a range that claims the ones before it are all there, and the resumed
//! request fetches them again from wherever they are held.

use crate::temporal::{TemporalQueryParameters, query::forwarding::ForwardedHistory};
use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
use andromeda_temporal::{error::TemporalError, service::TemporalRange};
use chrono::{DateTime, Utc};

/// How complete the history a temporal query answers with is.
#[derive(Debug)]
pub(crate) enum HistoryCompleteness {
    /// No context source contributed, so the local read's own range, if it
    /// cut the history short, is the response's.
    Local(Option<TemporalRange>),
    /// Context sources contributed, and every contributor answered in full.
    Complete,
    /// Context sources contributed, and at least one contributor stopped short.
    UpTo(HistoryBound),
}

/// The instant the merged history is complete up to, in the direction the
/// response is paged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryBound {
    /// The interval the query asked for, which fixes the range-start.
    interval: TimeInterval,
    /// The direction the rest of the history is paged in.
    direction: PageDirection,
    /// The last instant every contributor answered in full up to (forwards),
    /// or down to (backwards).
    bound: DateTime<Utc>,
}

impl HistoryCompleteness {
    /// How complete the merge of the local read and what `forwarded` holds is.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Query`] when the request's temporal window
    /// cannot be read back, which validation has already ruled out.
    pub(crate) fn of(
        params: &TemporalQueryParameters,
        local: Option<TemporalRange>,
        forwarded: Option<&ForwardedHistory>,
    ) -> Result<HistoryCompleteness, TemporalError> {
        let Some(history) = forwarded.filter(|history| history.holds_remote_entities()) else {
            return Ok(HistoryCompleteness::Local(local));
        };
        // Clause 6.3.10: "backwards" with `lastN`, "forwards" without it.
        let direction = params.last_n.map_or(PageDirection::Forward, |requested| PageDirection::Backward { requested });
        let local_stop = local.as_ref().and_then(|range| range.observed).map(|(least, most)| match direction {
            PageDirection::Forward => most,
            PageDirection::Backward { .. } => least,
        });
        let source_stops = history
            .sources
            .iter()
            .flat_map(|source| source.partial_ranges.iter().map(NgsiLdContentRange::end));
        let stops = local_stop.into_iter().chain(source_stops);
        let bound = match direction {
            PageDirection::Forward => stops.min(),
            PageDirection::Backward { .. } => stops.max(),
        };
        let Some(bound) = bound else {
            return Ok(HistoryCompleteness::Complete);
        };
        let interval = match local {
            Some(range) => range.interval,
            None => params.window()?.interval,
        };
        Ok(HistoryCompleteness::UpTo(HistoryBound { interval, direction, bound }))
    }

    /// Leave out of every merged entity the instances past the bound the
    /// history is complete up to.
    pub(crate) fn trim(&self, entities: &mut [TemporalHistory], timeproperty: TimeProp) {
        let HistoryCompleteness::UpTo(bound) = self else {
            return;
        };
        for entity in entities {
            entity.retain_instants(timeproperty, |instant| bound.admits(instant));
        }
    }

    /// The range the response reports for the `page` it answers with, or
    /// `None` when it answers with the whole history.
    pub(crate) fn range(self, page: &[TemporalHistory], timeproperty: TimeProp) -> Option<TemporalRange> {
        match self {
            HistoryCompleteness::Local(range) => range,
            HistoryCompleteness::Complete => None,
            HistoryCompleteness::UpTo(bound) => Some(TemporalRange {
                interval: bound.interval,
                observed: instant_span(page, timeproperty),
                direction: bound.direction,
            }),
        }
    }
}

/// The least and most recent instants any instance of any of `histories`
/// carries on `timeproperty`.
fn instant_span(histories: &[TemporalHistory], timeproperty: TimeProp) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    histories
        .iter()
        .filter_map(|history| history.instant_span(timeproperty))
        .fold(None, |span, (least, most)| {
            Some(span.map_or((least, most), |(held_least, held_most): (DateTime<Utc>, DateTime<Utc>)| {
                (held_least.min(least), held_most.max(most))
            }))
        })
}

impl HistoryBound {
    /// Whether an instance at `instant` lies on the complete side of the bound.
    fn admits(&self, instant: DateTime<Utc>) -> bool {
        match self.direction {
            PageDirection::Forward => instant <= self.bound,
            PageDirection::Backward { .. } => instant >= self.bound,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        temporal::query::{
            complete_history::{HistoryBound, HistoryCompleteness},
            forwarding::ForwardedHistory,
        },
        test_history::held_history,
    };
    use andromeda_federation::{entity_budget::EntityBudget, outcome::ForwardedTemporalEntities};
    use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
    use andromeda_ngsi_ld_lexicon::{
        aggr_method::AggrMethod,
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        time_prop::TimeProp,
        time_rel::TimeRel,
    };
    use andromeda_ngsi_ld_model::{
        entity_type::EntityTypeValue,
        registration::forwarding_mode::ForwardingMode,
        temporal::{StreamExtent, TemporalAggregationRow, aggregated_entity::AggregatedTemporalEntity, temporal_history::TemporalHistory},
    };
    use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use andromeda_temporal::service::TemporalRange;
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::num::NonZeroU32;

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    fn after_2020() -> TimeInterval {
        TimeInterval::After(at("2020-01-01T00:00:00Z"))
    }

    fn params(last_n: Option<NonZeroU32>) -> TemporalQueryParameters {
        TemporalQueryParameters {
            timerel: Some(TimeRel::After),
            time_at: Some(at("2020-01-01T00:00:00Z")),
            last_n,
            ..TemporalQueryParameters::default()
        }
    }

    /// A source answer holding one entity, cut at each range in `partial`.
    async fn source(partial: &[NgsiLdContentRange]) -> ForwardedTemporalEntities {
        ForwardedTemporalEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: vec![held_history(json!({"id": "urn:ngsi-ld:Vehicle:remote", "type": "Vehicle"})).await],
            warnings: Vec::new(),
            linked_map: None,
            partial_ranges: partial.to_vec(),
        }
    }

    fn forwarded(sources: Vec<ForwardedTemporalEntities>) -> ForwardedHistory {
        ForwardedHistory {
            sources,
            budget: EntityBudget::new(NonZeroU32::new(100).expect("a positive budget")),
        }
    }

    fn range(start: &str, end: &str) -> NgsiLdContentRange {
        NgsiLdContentRange::new(at(start), at(end), None)
    }

    fn local_cut(least: &str, most: &str, direction: PageDirection) -> TemporalRange {
        TemporalRange {
            interval: after_2020(),
            observed: Some((at(least), at(most))),
            direction,
        }
    }

    /// Without a contributing source the local read's own range stands.
    #[tokio::test]
    async fn without_a_contributing_source_the_local_range_stands() {
        let local = local_cut("2020-01-01T00:00:00Z", "2020-01-02T00:00:00Z", PageDirection::Forward);
        let completeness = HistoryCompleteness::of(&params(None), Some(local.clone()), None).expect("a readable window");
        assert!(matches!(completeness, HistoryCompleteness::Local(Some(ref kept)) if kept.observed == local.observed));
    }

    /// Clause 6.3.10: when every contributor answered in full, so does the
    /// response.
    #[tokio::test]
    async fn when_every_contributor_answered_in_full_the_history_is_complete() {
        let history = forwarded(vec![source(&[]).await]);
        let completeness = HistoryCompleteness::of(&params(None), None, Some(&history)).expect("a readable window");
        assert!(matches!(completeness, HistoryCompleteness::Complete));
        assert!(completeness.range(&[], TimeProp::Observed).is_none());
    }

    /// Forwards, the history is complete up to the earliest stop: the local
    /// cut's most recent instant and every source's range-end alike.
    #[tokio::test]
    async fn forwards_the_bound_is_the_earliest_stop_of_every_contributor() {
        let history = forwarded(vec![
            source(&[range("2020-01-01T00:00:00Z", "2020-01-05T00:00:00Z")]).await,
            source(&[range("2020-01-01T00:00:00Z", "2020-01-03T00:00:00Z")]).await,
        ]);
        let local = local_cut("2020-01-01T00:00:00Z", "2020-01-04T00:00:00Z", PageDirection::Forward);
        let completeness = HistoryCompleteness::of(&params(None), Some(local), Some(&history)).expect("a readable window");
        assert!(matches!(
            completeness,
            HistoryCompleteness::UpTo(HistoryBound { direction: PageDirection::Forward, bound, .. }) if bound == at("2020-01-03T00:00:00Z")
        ));
    }

    /// Backwards (with `lastN`), the history is complete down to the latest
    /// stop, and the range reports the requested `lastN`.
    #[tokio::test]
    async fn backwards_the_bound_is_the_latest_stop_of_every_contributor() {
        let history = forwarded(vec![source(&[range("2021-01-01T00:00:00Z", "2020-06-01T00:00:00Z")]).await]);
        let local = local_cut(
            "2020-03-01T00:00:00Z",
            "2020-12-01T00:00:00Z",
            PageDirection::Backward {
                requested: NonZeroU32::new(50).expect("a positive lastN"),
            },
        );
        let completeness = HistoryCompleteness::of(&params(NonZeroU32::new(50)), Some(local), Some(&history)).expect("a readable window");
        assert!(matches!(
            completeness,
            HistoryCompleteness::UpTo(HistoryBound { direction: PageDirection::Backward { requested }, bound, .. })
                if bound == at("2020-06-01T00:00:00Z") && requested.get() == 50
        ));
    }

    /// The instances past the bound are left out, and the range the response
    /// reports is the one the trimmed page carries.
    #[tokio::test]
    async fn instances_past_the_bound_are_left_out_and_the_range_is_the_page_s() {
        let history = forwarded(vec![source(&[range("2020-01-01T00:00:00Z", "2020-01-02T00:00:00Z")]).await]);
        let completeness = HistoryCompleteness::of(&params(None), None, Some(&history)).expect("a readable window");
        let mut page = vec![
            held_history(json!({
                "id": "urn:ngsi-ld:Vehicle:local",
                "type": "Vehicle",
                "https://example.com/speed": [
                    {"type": "Property", "value": 1, "observedAt": "2020-01-01T12:00:00Z"},
                    {"type": "Property", "value": 2, "observedAt": "2020-01-02T00:00:00Z"},
                    {"type": "Property", "value": 3, "observedAt": "2020-01-03T00:00:00Z"}
                ]
            }))
            .await,
        ];
        completeness.trim(&mut page, TimeProp::Observed);
        assert_eq!(page[0].instance_count(), 2, "{:?}", page[0]);
        let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
        assert_eq!(reported.observed, Some((at("2020-01-01T12:00:00Z"), at("2020-01-02T00:00:00Z"))));
        assert_eq!(reported.interval, after_2020());
        assert_eq!(reported.direction, PageDirection::Forward);
    }

    /// One hourly period of an aggregate from `hour` 2020-01-01, counting one
    /// instance.
    fn hourly(hour: i64) -> TemporalAggregationRow {
        let start = at("2020-01-01T00:00:00Z") + Duration::hours(hour);
        TemporalAggregationRow {
            attribute_name: "https://example.com/speed".parse().expect("valid attribute name"),
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
            total_count: Some(1),
            distinct_count: None,
            group_keys: Vec::new(),
            stream_extent: StreamExtent::Whole,
        }
    }

    /// Clause 6.3.10 over an aggregated merge: a source that stopped at 03:00
    /// leaves out the periods ending after it, and the range ends at the end
    /// of the last period kept.
    #[tokio::test]
    async fn an_aggregated_merge_keeps_the_whole_periods_up_to_the_bound() {
        let history = forwarded(vec![source(&[range("2020-01-01T00:00:00Z", "2020-01-01T03:00:00Z")]).await]);
        let completeness = HistoryCompleteness::of(&params(None), None, Some(&history)).expect("a readable window");
        let rows: Vec<TemporalAggregationRow> = (0..6).map(hourly).collect();
        let aggregated = AggregatedTemporalEntity::from_rows(
            "urn:ngsi-ld:Vehicle:local".parse().expect("an identifier"),
            EntityTypeValue::Single("Vehicle".parse().expect("a type")),
            &rows,
            &[AggrMethod::TotalCount],
            0,
            AttributeName::clone,
        );
        let mut page = vec![TemporalHistory::Aggregates(aggregated)];
        completeness.trim(&mut page, TimeProp::Observed);
        let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
        assert_eq!(reported.observed, Some((at("2020-01-01T00:00:00Z"), at("2020-01-01T03:00:00Z"))));
    }
}
