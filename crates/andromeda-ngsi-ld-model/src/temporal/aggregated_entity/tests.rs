use crate::{
    entity_type::EntityTypeValue,
    temporal::{
        AggregationAverage,
        AggregationMinMax,
        GroupKeyValue,
        TemporalAggregationRow,
        aggregated_entity::{AggregateValue, AggregatedTemporalEntity, AggregatedTriple, AggregatedValues, AggregationLayout},
    },
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::temporal::{
    period_stride::PeriodStride,
    period_timeline::{PeriodLimitExceeded, PeriodTimeline},
    window::TimeInterval,
};
use chrono::{DateTime, TimeDelta, Utc};

const WEIGHT: &str = "https://uri.etsi.org/ngsi-ld/default-context/weight";

fn instant(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
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
    }
}

fn laid_out(rows: &[TemporalAggregationRow], methods: &[AggrMethod], layout: AggregationLayout<'_>) -> Result<AggregatedTemporalEntity, PeriodLimitExceeded> {
    AggregatedTemporalEntity::from_rows(
        "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
        EntityTypeValue::Single("Vehicle".parse().expect("a type")),
        rows,
        methods,
        layout,
        AttributeName::clone,
    )
}

fn entity(rows: &[TemporalAggregationRow], methods: &[AggrMethod], depth: usize) -> AggregatedTemporalEntity {
    laid_out(
        rows,
        methods,
        AggregationLayout {
            grouping_depth: depth,
            timeline: None,
        },
    )
    .expect("no timeline to exceed")
}

/// An hourly timeline over 00:00 to 03:00 of the first day of 2026.
fn three_hours(maximum: u64) -> PeriodTimeline {
    PeriodTimeline::new(
        PeriodStride::Fixed(TimeDelta::hours(1)),
        TimeInterval::Between {
            from: instant("2026-01-01T00:00:00Z"),
            to: instant("2026-01-01T03:00:00Z"),
        },
        maximum,
    )
}

/// Clause 4.5.19: "as many array elements as there are periods in the time
/// range of the query", so the hour holding no instance has its triple. Its
/// count is zero and its average has nothing to be calculated over.
#[test]
fn a_period_holding_no_instance_has_its_triple() {
    let mut first = bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z");
    first.total_count = Some(1);
    let mut third = bucket("2026-01-01T02:00:00Z", "2026-01-01T03:00:00Z");
    third.total_count = Some(1);
    let timeline = three_hours(100);
    let aggregated = laid_out(
        &[first, third],
        &[AggrMethod::TotalCount, AggrMethod::Avg],
        AggregationLayout {
            grouping_depth: 0,
            timeline: Some(&timeline),
        },
    )
    .expect("within the maximum");
    let instances = &aggregated.attrs[&weight()];
    let AggregatedValues::Triples(counts) = &instances[0].aggregates[0].values else {
        panic!("totalCount is triples");
    };
    pretty_assertions::assert_eq!(counts.len(), 3);
    pretty_assertions::assert_eq!(
        counts[1],
        AggregatedTriple {
            value: AggregateValue::Count(0),
            start: Some(instant("2026-01-01T01:00:00Z")),
            end: Some(instant("2026-01-01T02:00:00Z")),
        }
    );
    let AggregatedValues::Triples(averages) = &instances[0].aggregates[1].values else {
        panic!("avg is triples");
    };
    pretty_assertions::assert_eq!(averages[1].value, AggregateValue::Absent);
}

#[test]
fn a_range_of_more_periods_than_the_timeline_answers_is_refused() {
    let timeline = three_hours(2);
    let refused = laid_out(
        &[bucket("2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")],
        &[AggrMethod::Avg],
        AggregationLayout {
            grouping_depth: 0,
            timeline: Some(&timeline),
        },
    );
    pretty_assertions::assert_eq!(refused, Err(PeriodLimitExceeded { periods: 3, maximum: 2 }));
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
