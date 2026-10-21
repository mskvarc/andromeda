use crate::{
    entity_type::EntityTypeValue,
    temporal::{
        AggregationAverage,
        AggregationMinMax,
        GroupKeyValue,
        StreamExtent,
        TemporalAggregationRow,
        aggregated_entity::{AggregateValue, AggregatedTemporalEntity, AggregatedTriple, AggregatedValues},
    },
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType, time::date_time::NgsiLdDateTime};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use chrono::{DateTime, Utc};

const WEIGHT: &str = "https://uri.etsi.org/ngsi-ld/default-context/weight";

fn instant(raw: &str) -> DateTime<Utc> {
    raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
}

fn weight() -> AttributeName {
    WEIGHT.parse().expect("a valid attribute name")
}

/// One period of the weight attribute, every aggregate absent.
fn bucket(start: &str, end: &str) -> TemporalAggregationRow {
    TemporalAggregationRow {
        attribute_name: weight(),
        attribute_type: AttributeType::Property,
        dataset_id: Some("urn:ngsi-ld:Dataset:a".parse().expect("a dataset identifier")),
        bucket_start: Some(instant(start)),
        bucket_end: Some(instant(end)),
        avg: None,
        min: None,
        max: None,
        sum: None,
        sumsq: None,
        stddev: None,
        total_count: None,
        distinct_count: None,
        group_keys: Vec::new(),
        stream_extent: StreamExtent::Whole,
    }
}

fn entity(rows: &[TemporalAggregationRow], methods: &[AggrMethod], depth: usize) -> AggregatedTemporalEntity {
    AggregatedTemporalEntity::from_rows(
        "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
        EntityTypeValue::Single("Vehicle".parse().expect("a type")),
        rows,
        methods,
        depth,
        AttributeName::clone,
    )
}

/// Clause 4.5.19 makes the first member of a triple "the value obtained after
/// applying the aggregation method over the period", which a period holding no
/// instance does not have, and clause 4.5.0 rules null out, so the hour between
/// the two stored ones has no triple for any method, a count included.
#[test]
fn a_period_holding_no_instance_has_no_triple() {
    let mut first = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    first.total_count = Some(1);
    first.avg = Some(AggregationAverage::Number(JsonNumber::from(10)));
    let mut third = bucket("2026-01-01T02:00:00Z", "2026-01-01T03:00:00Z");
    third.total_count = Some(1);
    third.avg = Some(AggregationAverage::Number(JsonNumber::from(20)));
    let aggregated = entity(&[first, third], &[AggrMethod::TotalCount, AggrMethod::Avg], 0);
    let instances = &aggregated.attrs[&weight()];
    let AggregatedValues::Triples(counts) = &instances[0].aggregates[0].values else {
        panic!("totalCount is triples");
    };
    pretty_assertions::assert_eq!(counts.len(), 2);
    let AggregatedValues::Triples(averages) = &instances[0].aggregates[1].values else {
        panic!("avg is triples");
    };
    pretty_assertions::assert_eq!(
        averages,
        &vec![
            AggregatedTriple {
                value: AggregateValue::Number(JsonNumber::from(10)),
                start: Some(instant("2026-01-01T00:00:00Z")),
                end: Some(instant("2026-01-01T01:00:00Z")),
            },
            AggregatedTriple {
                value: AggregateValue::Number(JsonNumber::from(20)),
                start: Some(instant("2026-01-01T02:00:00Z")),
                end: Some(instant("2026-01-01T03:00:00Z")),
            },
        ]
    );
}

/// Clause 4.5.19: one triple per period and method, the exact numbers kept.
#[test]
fn aggregated_rows_become_typed_triples() {
    let mut first = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    first.avg = Some(AggregationAverage::Number(JsonNumber::from(2)));
    first.max = Some(AggregationMinMax::Number(JsonNumber::from(9)));
    let mut second = bucket("2026-01-01T01:00:00Z", "2026-01-01T02:00:00Z");
    second.avg = Some(AggregationAverage::Number(JsonNumber::from(3)));
    second.max = Some(AggregationMinMax::Number(JsonNumber::from(8)));
    let aggregated = entity(&[first, second], &[AggrMethod::Avg, AggrMethod::Max], 0);
    let instances = &aggregated.attrs[&weight()];
    pretty_assertions::assert_eq!(instances.len(), 1);
    pretty_assertions::assert_eq!(instances[0].aggregates.len(), 2);
    let AggregatedValues::Triples(avg) = &instances[0].aggregates[0].values else {
        panic!("avg is triples");
    };
    pretty_assertions::assert_eq!(avg.len(), 2);
    pretty_assertions::assert_eq!(avg[1].value, AggregateValue::Number(JsonNumber::from(3)));
    pretty_assertions::assert_eq!(avg[0].start, Some(instant("2026-01-01T00:00:00Z")));
}

/// The grouped extension nests the triples by group key, in the order of the
/// rows the store returned.
#[test]
fn grouped_aggregation_nests_by_group_key() {
    let mut kilograms = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    kilograms.total_count = Some(2);
    kilograms.group_keys = vec![GroupKeyValue::Value("KGM".to_owned())];
    let mut absent = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    absent.total_count = Some(1);
    absent.group_keys = vec![GroupKeyValue::Absent];
    let aggregated = entity(&[kilograms, absent], &[AggrMethod::TotalCount], 1);
    let AggregatedValues::Grouped(groups) = &aggregated.attrs[&weight()][0].aggregates[0].values else {
        panic!("the values are grouped");
    };
    pretty_assertions::assert_eq!(
        groups.keys().cloned().collect::<Vec<_>>(),
        vec![GroupKeyValue::Value("KGM".to_owned()), GroupKeyValue::Absent]
    );
}

/// Instances of one attribute are told apart by `datasetId`.
#[test]
fn two_datasets_are_two_aggregated_instances() {
    let mut default_instance = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    default_instance.dataset_id = None;
    let named = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    let aggregated = entity(&[default_instance, named], &[AggrMethod::TotalCount], 0);
    pretty_assertions::assert_eq!(aggregated.attrs[&weight()].len(), 2);
    assert!(aggregated.attrs[&weight()][0].dataset_id.is_none());
}

/// Hourly periods of the weight attribute from midnight, `from..to` hours,
/// each counting one instance.
fn hours(from: u32, to: u32) -> Vec<TemporalAggregationRow> {
    (from..to)
        .map(|hour| {
            let mut row = bucket(&format!("2026-01-01T{hour:02}:00:00Z"), &format!("2026-01-01T{:02}:00:00Z", hour + 1));
            row.total_count = Some(1);
            row
        })
        .collect()
}

/// Clause 6.3.10 reports an aggregated page by its periods: the least recent
/// start and the most recent end of any of them.
#[test]
fn the_period_span_runs_from_the_first_start_to_the_last_end() {
    let aggregated = entity(&hours(2, 7), &[AggrMethod::TotalCount], 0);
    pretty_assertions::assert_eq!(
        aggregated.period_span(),
        Some((instant("2026-01-01T02:00:00Z"), instant("2026-01-01T07:00:00Z")))
    );
    pretty_assertions::assert_eq!(entity(&[], &[AggrMethod::TotalCount], 0).period_span(), None);
}

/// A merged page complete up to 05:00 keeps the periods lying wholly before
/// it, at whatever depth a grouping nests them.
#[test]
fn only_the_periods_wholly_on_the_admitted_side_are_retained() {
    let bound = instant("2026-01-01T05:00:00Z");
    for depth in [0, 1] {
        let mut rows = hours(2, 7);
        for row in &mut rows {
            row.group_keys = vec![GroupKeyValue::Value("KGM".to_owned())];
        }
        let mut aggregated = entity(&rows, &[AggrMethod::TotalCount], depth);
        aggregated.retain_periods(|at| at <= bound);
        pretty_assertions::assert_eq!(aggregated.period_span(), Some((instant("2026-01-01T02:00:00Z"), bound)), "depth {depth}");
    }
}

/// Clause 4.11: a source's aggregate joins with only the periods lying wholly
/// in the requested window, a period ending on the window's closing instant
/// included, at whatever depth a grouping nests them.
#[test]
fn only_the_periods_lying_wholly_in_the_window_are_retained() {
    let window = TimeInterval::Between {
        from: instant("2026-01-01T03:00:00Z"),
        to: instant("2026-01-01T05:00:00Z"),
    };
    for depth in [0, 1] {
        let mut rows = hours(2, 7);
        for row in &mut rows {
            row.group_keys = vec![GroupKeyValue::Value("KGM".to_owned())];
        }
        let mut aggregated = entity(&rows, &[AggrMethod::TotalCount], depth);
        aggregated.retain_periods_within(window);
        pretty_assertions::assert_eq!(
            aggregated.period_span(),
            Some((instant("2026-01-01T03:00:00Z"), instant("2026-01-01T05:00:00Z"))),
            "depth {depth}"
        );
    }
    let mut outside = entity(&hours(2, 7), &[AggrMethod::TotalCount], 0);
    outside.retain_periods_within(TimeInterval::After(instant("2026-01-02T00:00:00Z")));
    assert!(outside.attrs.is_empty(), "an attribute left without a period is removed");
    pretty_assertions::assert_eq!(outside.period_span(), None);
    let mut unbounded = entity(&hours(2, 7), &[AggrMethod::TotalCount], 0);
    unbounded.retain_periods_within(TimeInterval::Unbounded);
    pretty_assertions::assert_eq!(
        unbounded.period_span(),
        Some((instant("2026-01-01T02:00:00Z"), instant("2026-01-01T07:00:00Z")))
    );
}
