use crate::{
    temporal::complete_history::{HistoryBound, HistoryCompleteness, Stop, StopSide},
    test_history::held_history,
};
use andromeda_federation::outcome::ForwardedTemporalEntities;
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::WarnCode};
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    time::date_time::NgsiLdDateTime,
    time_prop::TimeProp,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::{
    entity_type::EntityTypeValue,
    registration::forwarding_mode::ForwardingMode,
    temporal::{StreamExtent, TemporalAggregationRow, aggregated_entity::AggregatedTemporalEntity, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_query::temporal::{continuation::TemporalContinuation, instance_page::PageDirection, window::TimeInterval};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::service::TemporalRange;
use chrono::{DateTime, Duration, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::num::NonZeroU32;

fn at(raw: &str) -> DateTime<Utc> {
    raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
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

fn backward_50() -> PageDirection {
    PageDirection::Backward {
        requested: NonZeroU32::new(50).expect("a positive lastN"),
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

fn range(start: &str, end: &str) -> NgsiLdContentRange {
    NgsiLdContentRange::new(at(start), at(end), None)
}

fn local_cut(least: &str, most: &str, direction: PageDirection) -> TemporalRange {
    TemporalRange {
        interval: after_2020(),
        observed: Some((at(least), at(most))),
        direction,
        next: None,
    }
}

fn speed(observed: &str) -> Value {
    json!({"type": "Property", "value": 1, "observedAt": observed})
}

/// The merged history of one entity whose `speed` instances were observed at
/// each instant in `instants`.
async fn merged(instants: &[&str]) -> Vec<TemporalHistory> {
    let instances: Vec<Value> = instants.iter().map(|instant| speed(instant)).collect();
    vec![
        held_history(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "https://example.com/speed": instances
        }))
        .await,
    ]
}

fn stop_of(completeness: &HistoryCompleteness) -> Option<Stop> {
    match completeness {
        HistoryCompleteness::UpTo(HistoryBound { stop, .. }) => Some(*stop),
        HistoryCompleteness::Local(_) | HistoryCompleteness::Complete => None,
    }
}

/// Without a contributing source the local read's own range stands.
#[tokio::test]
async fn without_a_contributing_source_the_local_range_stands() {
    let local = local_cut("2020-01-01T00:00:00Z", "2020-01-02T00:00:00Z", PageDirection::Forward);
    let completeness = HistoryCompleteness::of(&params(None), Some(local.clone()), &[]).expect("a readable window");
    assert!(matches!(completeness, HistoryCompleteness::Local(Some(ref kept)) if kept.observed == local.observed));
}

/// Clause 6.3.10: when every contributor answered in full, so does the
/// response.
#[tokio::test]
async fn when_every_contributor_answered_in_full_the_history_is_complete() {
    let sources = [source(&[]).await];
    let completeness = HistoryCompleteness::of(&params(None), None, &sources).expect("a readable window");
    assert!(matches!(completeness, HistoryCompleteness::Complete));
    assert!(completeness.range(&[], TimeProp::Observed).is_none());
}

/// Forwards, the history is complete up to the earliest stop: the local
/// cut's most recent instant and every source's range-end alike.
#[tokio::test]
async fn forwards_the_bound_is_the_earliest_stop_of_every_contributor() {
    let sources = [
        source(&[range("2020-01-01T00:00:00Z", "2020-01-05T00:00:00Z")]).await,
        source(&[range("2020-01-01T00:00:00Z", "2020-01-03T00:00:00Z")]).await,
    ];
    let local = local_cut("2020-01-01T00:00:00Z", "2020-01-04T00:00:00Z", PageDirection::Forward);
    let completeness = HistoryCompleteness::of(&params(None), Some(local), &sources).expect("a readable window");
    assert_eq!(
        stop_of(&completeness),
        Some(Stop {
            at: at("2020-01-03T00:00:00Z"),
            side: StopSide::Exclusive,
        })
    );
}

/// Backwards (with `lastN`), the history is complete down to the latest
/// stop, and the range reports the requested `lastN`.
#[tokio::test]
async fn backwards_the_bound_is_the_latest_stop_of_every_contributor() {
    let sources = [source(&[range("2021-01-01T00:00:00Z", "2020-06-01T00:00:00Z")]).await];
    let local = local_cut("2020-03-01T00:00:00Z", "2020-12-01T00:00:00Z", backward_50());
    let completeness = HistoryCompleteness::of(&params(NonZeroU32::new(50)), Some(local), &sources).expect("a readable window");
    assert!(matches!(
        completeness,
        HistoryCompleteness::UpTo(HistoryBound { direction: PageDirection::Backward { requested }, stop, .. })
            if stop.at == at("2020-06-01T00:00:00Z") && requested.get() == 50
    ));
}

/// A source's range-end does not say every instance at it was served, so
/// the instances at 01:10 are left to the continuation, which opens past the
/// last instant kept.
#[tokio::test]
async fn a_source_stop_before_the_local_one_is_an_exclusive_bound() {
    let sources = [source(&[range("2020-01-01T00:00:00Z", "2020-01-01T01:10:00Z")]).await];
    let local = local_cut("2020-01-01T01:00:00Z", "2020-01-01T01:19:00Z", PageDirection::Forward);
    let mut completeness = HistoryCompleteness::of(&params(None), Some(local), &sources).expect("a readable window");
    let mut page = merged(&["2020-01-01T01:05:00Z", "2020-01-01T01:10:00Z", "2020-01-01T01:10:00Z", "2020-01-01T01:15:00Z"]).await;
    let warnings = completeness.trim(&mut page, TimeProp::Observed);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(page[0].instance_count(), 1, "{:?}", page[0]);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(reported.observed, Some((at("2020-01-01T01:05:00Z"), at("2020-01-01T01:05:00Z"))));
    assert_eq!(
        reported.next,
        Some(TemporalContinuation {
            interval: TimeInterval::After(at("2020-01-01T01:05:00.000001Z")),
            last_n: None,
        })
    );
}

/// The local page carries its edge instant whole, so both holders'
/// instances at 01:10 stay.
#[tokio::test]
async fn a_local_stop_before_a_source_one_is_an_inclusive_bound() {
    let sources = [source(&[range("2020-01-01T00:00:00Z", "2020-01-01T01:19:00Z")]).await];
    let local = local_cut("2020-01-01T01:00:00Z", "2020-01-01T01:10:00Z", PageDirection::Forward);
    let mut completeness = HistoryCompleteness::of(&params(None), Some(local), &sources).expect("a readable window");
    let mut page = merged(&["2020-01-01T01:05:00Z", "2020-01-01T01:10:00Z", "2020-01-01T01:10:00Z", "2020-01-01T01:15:00Z"]).await;
    let warnings = completeness.trim(&mut page, TimeProp::Observed);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(page[0].instance_count(), 3, "{:?}", page[0]);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(
        reported.next.map(|next| next.interval),
        Some(TimeInterval::After(at("2020-01-01T01:10:00.000001Z")))
    );
}

/// Of two stops at one instant the exclusive one admits less, and wins.
#[tokio::test]
async fn a_local_and_a_source_stop_at_one_instant_are_an_exclusive_bound() {
    let sources = [source(&[range("2020-01-01T00:00:00Z", "2020-01-01T01:10:00Z")]).await];
    let local = local_cut("2020-01-01T01:00:00Z", "2020-01-01T01:10:00Z", PageDirection::Forward);
    let completeness = HistoryCompleteness::of(&params(None), Some(local), &sources).expect("a readable window");
    assert_eq!(
        stop_of(&completeness),
        Some(Stop {
            at: at("2020-01-01T01:10:00Z"),
            side: StopSide::Exclusive,
        })
    );
}

/// Every instance of the page lies at the source's stop, so leaving them out
/// would leave the page nothing to advance by: they stay, and the source is
/// reported for what it may hold there beyond its page (clause 6.3.17).
#[tokio::test]
async fn an_exclusive_bound_that_would_leave_nothing_is_taken_inclusively_with_a_warning() {
    let sources = [source(&[range("2020-01-01T00:00:00Z", "2020-01-01T01:10:00Z")]).await];
    let mut completeness = HistoryCompleteness::of(&params(None), None, &sources).expect("a readable window");
    let mut page = merged(&["2020-01-01T01:10:00Z", "2020-01-01T01:10:00Z"]).await;
    let warnings = completeness.trim(&mut page, TimeProp::Observed);
    assert_eq!(page[0].instance_count(), 2, "{:?}", page[0]);
    assert!(
        matches!(warnings.as_slice(), [warning] if warning.code() == WarnCode::RevalidationFailed && warning.text().as_str().contains("urn:ngsi-ld:ContextSourceRegistration:1")),
        "{warnings:?}"
    );
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(
        reported.next.map(|next| next.interval),
        Some(TimeInterval::After(at("2020-01-01T01:10:00.000001Z")))
    );
}

/// Backwards the source's stop at 01:10 is the latest, and exclusive: only
/// 01:15 stays, and the continuation closes before it asking for the 49
/// instances of `lastN` the one stream is still owed.
#[tokio::test]
async fn backwards_a_source_stop_after_the_local_one_is_an_exclusive_bound() {
    let sources = [source(&[range("2020-01-01T02:00:00Z", "2020-01-01T01:10:00Z")]).await];
    let local = local_cut("2020-01-01T01:01:00Z", "2020-01-01T01:20:00Z", backward_50());
    let mut completeness = HistoryCompleteness::of(&params(NonZeroU32::new(50)), Some(local), &sources).expect("a readable window");
    let mut page = merged(&["2020-01-01T01:05:00Z", "2020-01-01T01:10:00Z", "2020-01-01T01:15:00Z"]).await;
    let warnings = completeness.trim(&mut page, TimeProp::Observed);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(page[0].instance_count(), 1, "{:?}", page[0]);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(reported.observed, Some((at("2020-01-01T01:15:00Z"), at("2020-01-01T01:15:00Z"))));
    assert_eq!(
        reported.next,
        Some(TemporalContinuation {
            interval: TimeInterval::Between {
                from: at("2020-01-01T00:00:00Z"),
                to: at("2020-01-01T01:15:00Z"),
            },
            last_n: NonZeroU32::new(49),
        })
    );
}

/// The `speed` stream of dataset `old` lies wholly past the bound, so it was
/// served nothing and the continuation asks for the whole `lastN` again.
#[tokio::test]
async fn a_dataset_stream_the_bound_leaves_empty_counts_as_served_nothing() {
    let sources = [source(&[range("2020-01-01T02:00:00Z", "2020-01-01T01:10:00Z")]).await];
    let mut completeness = HistoryCompleteness::of(&params(NonZeroU32::new(50)), None, &sources).expect("a readable window");
    let mut page = vec![
        held_history(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "https://example.com/speed": [
                speed("2020-01-01T01:15:00Z"),
                speed("2020-01-01T01:16:00Z"),
                {"type": "Property", "value": 2, "observedAt": "2020-01-01T01:05:00Z", "datasetId": "urn:ngsi-ld:Dataset:old"}
            ]
        }))
        .await,
    ];
    completeness.trim(&mut page, TimeProp::Observed);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(reported.next.and_then(|next| next.last_n), NonZeroU32::new(50));
}

/// The instances past the bound are left out, and the range the response
/// reports is the one the trimmed page carries.
#[tokio::test]
async fn instances_past_the_bound_are_left_out_and_the_range_is_the_page_s() {
    let sources = [source(&[range("2020-01-01T00:00:00Z", "2020-01-02T00:00:00Z")]).await];
    let mut completeness = HistoryCompleteness::of(&params(None), None, &sources).expect("a readable window");
    let mut page = merged(&["2020-01-01T12:00:00Z", "2020-01-02T00:00:00Z", "2020-01-03T00:00:00Z"]).await;
    completeness.trim(&mut page, TimeProp::Observed);
    assert_eq!(page[0].instance_count(), 1, "{:?}", page[0]);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(reported.observed, Some((at("2020-01-01T12:00:00Z"), at("2020-01-01T12:00:00Z"))));
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

/// Six hourly periods of one local entity from midnight.
fn aggregated_page() -> Vec<TemporalHistory> {
    let rows: Vec<TemporalAggregationRow> = (0..6).map(hourly).collect();
    let aggregated = AggregatedTemporalEntity::from_rows(
        "urn:ngsi-ld:Vehicle:local".parse().expect("an identifier"),
        EntityTypeValue::Single("Vehicle".parse().expect("a type")),
        &rows,
        &[AggrMethod::TotalCount],
        0,
        AttributeName::clone,
    );
    vec![TemporalHistory::Aggregates(aggregated)]
}

/// Clause 6.3.10 over an aggregated merge: a source that stopped at 03:00
/// leaves out the periods ending after it, the period ending on it whole, and
/// the range ends at the end of the last period kept, where the continuation
/// opens.
#[tokio::test]
async fn an_aggregated_merge_keeps_the_whole_periods_up_to_the_bound() {
    let sources = [source(&[range("2020-01-01T00:00:00Z", "2020-01-01T03:00:00Z")]).await];
    let mut completeness = HistoryCompleteness::of(&params(None), None, &sources).expect("a readable window");
    let mut page = aggregated_page();
    completeness.trim(&mut page, TimeProp::Observed);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(reported.observed, Some((at("2020-01-01T00:00:00Z"), at("2020-01-01T03:00:00Z"))));
    assert_eq!(reported.next.map(|next| next.interval), Some(TimeInterval::After(at("2020-01-01T03:00:00Z"))));
}

/// Backwards the periods from 03:00 on stay, the one starting on the bound
/// whole; what they aggregate per stream is not known after the merge, so the
/// rest of the `lastN` cannot be stated and no continuation is offered.
#[tokio::test]
async fn a_backward_aggregated_merge_keeps_an_inclusive_bound_and_offers_no_continuation() {
    let sources = [source(&[range("2020-01-01T06:00:00Z", "2020-01-01T03:00:00Z")]).await];
    let mut completeness = HistoryCompleteness::of(&params(NonZeroU32::new(50)), None, &sources).expect("a readable window");
    let mut page = aggregated_page();
    completeness.trim(&mut page, TimeProp::Observed);
    let reported = completeness.range(&page, TimeProp::Observed).expect("a partial response");
    assert_eq!(reported.observed, Some((at("2020-01-01T03:00:00Z"), at("2020-01-01T06:00:00Z"))));
    assert_eq!(reported.next, None);
}
