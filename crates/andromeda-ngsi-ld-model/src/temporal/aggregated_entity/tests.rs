use crate::{
    entity_type::EntityTypeValue,
    temporal::{
        AggregationAverage,
        AggregationMinMax,
        GroupKeyValue,
        TemporalAggregationRow,
        aggregated_entity::{AggregateValue, AggregatedTemporalEntity, AggregatedValues},
    },
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_number::JsonNumber;
use chrono::{DateTime, Utc};

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
