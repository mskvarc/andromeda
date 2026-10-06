use crate::{
    context::ResponseContext,
    response::ResponseRenderer,
    structural_projection::StructuralProjection,
    temporal::temporal_response::{TemporalPayload, TemporalRepresentation, TemporalRoot},
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    spec_version::SpecVersion,
    temporal_format::TemporalFormat,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::{
    entity_type::EntityTypeValue,
    temporal::{
        AggregationAverage,
        AggregationMinMax,
        GroupKeyValue,
        TemporalAggregationRow,
        aggregated_entity::AggregatedTemporalEntity,
        temporal_attribute::TemporalAttribute,
        temporal_body::TemporalBody,
        temporal_entity::TemporalEntity,
    },
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json, value::RawValue};
use std::str::FromStr;

/// The evolution of one vehicle holding `members`.
fn evolution(members: Value) -> TemporalBody {
    let mut document = members;
    document["id"] = json!("urn:ngsi-ld:Vehicle:1");
    document["type"] = json!("Vehicle");
    let raw = RawValue::from_string(document.to_string()).expect("raw JSON");
    TemporalBody::Evolution(TemporalEntity::from_node(JsonPiece::new(&raw)).expect("an evolution"))
}

/// The representation a request over `format` and `timeproperty` asks for.
const fn representation(format: TemporalFormat, timeproperty: TimeProp) -> TemporalRepresentation {
    TemporalRepresentation { format, timeproperty }
}

/// The body one evolution is written to, as parsed JSON.
fn written(body: &TemporalBody, representation: TemporalRepresentation, version: Option<SpecVersion>, lang: Option<&LanguageFilter>) -> Value {
    written_with(&TemporalRoot::new(body, None), representation, version, lang)
}

/// The body one root is written to, as parsed JSON.
fn written_with(root: &TemporalRoot<'_>, representation: TemporalRepresentation, version: Option<SpecVersion>, lang: Option<&LanguageFilter>) -> Value {
    serde_json::from_slice(&written_bytes(root, representation, version, lang)).expect("the body is JSON")
}

/// The raw bytes one root is written as.
fn written_bytes(root: &TemporalRoot<'_>, representation: TemporalRepresentation, version: Option<SpecVersion>, lang: Option<&LanguageFilter>) -> Vec<u8> {
    let mut bytes = Vec::new();
    ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent)
        .with_requested_version(version)
        .with_temporal(TemporalPayload::Single(*root), representation, lang)
        .render_into(&mut bytes)
        .expect("the evolution is written");
    bytes
}

const NORMALIZED: TemporalRepresentation = representation(TemporalFormat::Normalized, TimeProp::Observed);

const VALUES: TemporalRepresentation = representation(TemporalFormat::TemporalValues, TimeProp::Observed);

/// Clause 4.5.7: every attribute is the array of its instances, even one.
#[test]
fn every_attribute_is_written_as_an_array() {
    let body = evolution(json!({"speed": {"type": "Property", "value": 25, "observedAt": "2025-01-01T00:00:00Z"}}));
    assert_eq!(
        written(&body, NORMALIZED, None, None)["speed"],
        json!([{"type": "Property", "value": 25, "observedAt": "2025-01-01T00:00:00Z"}])
    );
}

/// Clause 4.5.0: a deleted instance is its family's null encoding and the
/// `deletedAt` it is deleted at.
#[test]
fn a_tombstone_is_written_as_the_family_null() {
    let body = evolution(json!({
        "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2025-01-01T00:00:00Z"}],
        "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:null", "deletedAt": "2025-01-01T00:00:00Z"}],
        "name": [{"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}, "deletedAt": "2025-01-01T00:00:00Z"}],
        "tags": [{"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"], "deletedAt": "2025-01-01T00:00:00Z"}]
    }));
    let result = written(&body, NORMALIZED, None, None);
    assert_eq!(result["speed"][0]["value"], json!("urn:ngsi-ld:null"));
    assert_eq!(result["owner"][0]["object"], json!("urn:ngsi-ld:null"));
    assert_eq!(result["name"][0]["languageMap"], json!({"@none": "urn:ngsi-ld:null"}));
    assert_eq!(result["tags"][0]["valueList"], json!(["urn:ngsi-ld:null"]));
    for name in ["speed", "owner", "name", "tags"] {
        assert_eq!(result[name][0]["deletedAt"], json!("2025-01-01T00:00:00Z"), "{name}");
    }
}

/// Clause 4.5.9: a pair carries the instant on the Temporal Property the query
/// ran on, so a query over `modifiedAt` pairs each value with its `modifiedAt`.
#[test]
fn temporal_values_pair_by_the_timeproperty() {
    let body = evolution(json!({"currentStatus": [
        {"type": "Property", "value": "inTransitTo", "createdAt": "2025-01-01T00:00:00Z", "modifiedAt": "2025-01-02T00:00:00Z"},
        {"type": "Property", "value": "stoppedAt", "observedAt": "2025-01-03T00:00:00Z", "createdAt": "2025-01-01T01:00:00Z", "modifiedAt": "2025-01-02T01:00:00Z"}
    ]}));
    for (timeproperty, expected) in [
        (
            TimeProp::Modified,
            json!([["inTransitTo", "2025-01-02T00:00:00Z"], ["stoppedAt", "2025-01-02T01:00:00Z"]]),
        ),
        (
            TimeProp::Created,
            json!([["inTransitTo", "2025-01-01T00:00:00Z"], ["stoppedAt", "2025-01-01T01:00:00Z"]]),
        ),
        (TimeProp::Observed, json!([["inTransitTo", null], ["stoppedAt", "2025-01-03T00:00:00Z"]])),
    ] {
        let result = written(&body, representation(TemporalFormat::TemporalValues, timeproperty), None, None);
        assert_eq!(result["currentStatus"], json!({"type": "Property", "values": expected}), "{timeproperty}");
    }
}

/// Clause 4.5.9: one object per `datasetId`, the `datasetId` on the named one.
#[test]
fn temporal_values_split_by_dataset() {
    let body = evolution(json!({"speed": [
        {"type": "Property", "value": 25, "observedAt": "2025-01-01T00:00:00Z"},
        {"type": "Property", "value": 30, "observedAt": "2025-01-01T01:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:a"},
        {"type": "Property", "value": 26, "observedAt": "2025-01-01T02:00:00Z"}
    ]}));
    assert_eq!(
        written(&body, VALUES, None, None)["speed"],
        json!([
            {"type": "Property", "values": [[25, "2025-01-01T00:00:00Z"], [26, "2025-01-01T02:00:00Z"]]},
            {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:a", "values": [[30, "2025-01-01T01:00:00Z"]]}
        ])
    );
}

/// Clause 4.5.9 keeps every family on the plural member it names, three of
/// them wrapping their value in a one-member object.
#[test]
fn temporal_values_of_each_family_use_its_plural_member() {
    let at = "2025-01-01T00:00:00Z";
    let body = evolution(json!({
        "speed": [{"type": "Property", "value": 25, "observedAt": at}],
        "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "observedAt": at}],
        "spouse": [{"type": "Relationship", "object": "urn:ngsi-ld:Person:1", "observedAt": at}],
        "says": [{"type": "LanguageProperty", "languageMap": {"en": "yes"}, "observedAt": at}],
        "gender": [{"type": "VocabProperty", "vocab": "Male", "observedAt": at}],
        "tickets": [{"type": "JsonProperty", "json": {"id": "1"}, "observedAt": at}],
        "period": [{"type": "ListProperty", "valueList": ["First", "Second"], "observedAt": at}],
        "members": [{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Person:A"}, {"object": "urn:ngsi-ld:Person:B"}], "observedAt": at}],
        "devices": [{"type": "Relationship", "object": ["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"], "observedAt": at}]
    }));
    let result = written(&body, VALUES, None, None);
    assert_eq!(result["speed"]["values"], json!([[25, at]]));
    assert_eq!(result["location"]["values"], json!([[{"type": "Point", "coordinates": [1.0, 2.0]}, at]]));
    assert_eq!(result["spouse"]["objects"], json!([["urn:ngsi-ld:Person:1", at]]));
    assert_eq!(result["says"]["languageMaps"], json!([[{"languageMap": {"en": "yes"}}, at]]));
    assert_eq!(result["gender"]["vocabs"], json!([[{"vocab": "Male"}, at]]));
    assert_eq!(result["tickets"]["jsons"], json!([[{"json": {"id": "1"}}, at]]));
    assert_eq!(result["period"]["valueLists"], json!([[["First", "Second"], at]]));
    assert_eq!(
        result["members"]["objectLists"],
        json!([[["urn:ngsi-ld:Person:A", "urn:ngsi-ld:Person:B"], at]])
    );
    assert_eq!(result["devices"]["objects"], json!([[["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"], at]]));
    for (name, member) in [("period", "value"), ("members", "object"), ("members", "objects"), ("period", "values")] {
        assert!(result[name].get(member).is_none(), "{name} writes no {member}: {}", result[name]);
    }
}

/// Clause 4.5.9 with clause 6.3.10: an attribute whose instances all lie past
/// the page's cut is still represented, by its family's member holding no pair.
#[test]
fn an_emptied_attribute_writes_an_empty_sequence() {
    let TemporalBody::Evolution(mut entity) = evolution(json!({"fuel": [{"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"}]})) else {
        panic!("an evolution is built");
    };
    entity.attrs.insert(
        AttributeName::from_str("speed").expect("a name"),
        TemporalAttribute::emptied(AttributeType::Property),
    );
    entity.attrs.insert(
        AttributeName::from_str("isParked").expect("a name"),
        TemporalAttribute::emptied(AttributeType::Relationship),
    );
    let body = TemporalBody::Evolution(entity);
    let result = written(&body, VALUES, None, None);
    assert_eq!(result["speed"], json!({"type": "Property", "values": []}));
    assert_eq!(result["isParked"], json!({"type": "Relationship", "objects": []}));
    assert_eq!(written(&body, NORMALIZED, None, None)["speed"], json!([]));
}

/// Clause 4.5.9: a deletion has no instant on an observation axis, so its
/// `deletedAt` stands in for it.
#[test]
fn a_deletion_pairs_with_deleted_at() {
    let body = evolution(json!({
        "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2025-01-01T00:00:00Z"}],
        "period": [{"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"], "deletedAt": "2025-01-02T00:00:00Z"}]
    }));
    let result = written(&body, VALUES, None, None);
    assert_eq!(result["speed"]["values"], json!([["urn:ngsi-ld:null", "2025-01-01T00:00:00Z"]]));
    assert_eq!(result["period"]["valueLists"], json!([[["urn:ngsi-ld:null"], "2025-01-02T00:00:00Z"]]));
}

/// Clause 4.15: a `LanguageProperty` negotiated against `lang=en` is written
/// as a Property holding the string and the `lang` it is in, each instance.
#[test]
fn lang_reduces_language_properties() {
    let body = evolution(json!({"label": [
        {"type": "LanguageProperty", "languageMap": {"en": "Hello", "fr": "Bonjour"}, "observedAt": "2025-01-01T00:00:00Z"},
        {"type": "LanguageProperty", "languageMap": {"en": "Hi", "fr": "Salut"}, "observedAt": "2025-01-02T00:00:00Z"}
    ]}));
    let filter = LanguageFilter::parse("en").expect("a language filter");
    assert_eq!(
        written(&body, NORMALIZED, None, Some(&filter))["label"],
        json!([
            {"type": "Property", "value": "Hello", "lang": "en", "observedAt": "2025-01-01T00:00:00Z"},
            {"type": "Property", "value": "Hi", "lang": "en", "observedAt": "2025-01-02T00:00:00Z"}
        ])
    );
}

/// The presentation boundary decodes a Relationship target once.
#[test]
fn targets_are_percent_decoded_once() {
    let body = evolution(json!({"owner": [{"type": "Relationship", "object": "urn:a%3Ab", "observedAt": "2025-01-01T00:00:00Z"}]}));
    assert_eq!(written(&body, NORMALIZED, None, None)["owner"][0]["object"], json!("urn:a:b"));
    assert_eq!(written(&body, VALUES, None, None)["owner"]["objects"][0][0], json!("urn:a:b"));
}

/// Clause 4.3.6.8 over a Temporal Evolution: the array is the sequence of
/// instances in time, not the `datasetId` set, so a 1.0 target keeps every
/// instance and the `observedAt` that places it, and loses the `datasetId`.
#[test]
fn a_conformance_target_keeps_the_sequence() {
    let body = evolution(json!({"speed": [
        {"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:a"},
        {"type": "Property", "value": 2, "observedAt": "2025-01-02T00:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:a"},
        {"type": "Property", "value": 3, "observedAt": "2025-01-03T00:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:a"}
    ]}));
    let result = written(&body, NORMALIZED, Some(SpecVersion::V1_0), None);
    let instances = result["speed"].as_array().expect("an array of instances");
    assert_eq!(instances.len(), 3);
    for instance in instances {
        assert!(instance.get("observedAt").is_some(), "{instance}");
        assert!(instance.get("datasetId").is_none(), "{instance}");
    }
}

/// Clause 4.3.6.8: a `LanguageProperty` the target never defined is a Property
/// holding the map, which renames its plural member in the simplified
/// representation and writes the bare map.
#[test]
fn a_conformance_target_renames_the_plural_member() {
    let body = evolution(json!({"says": [{"type": "LanguageProperty", "languageMap": {"en": "yes"}, "observedAt": "2025-01-01T00:00:00Z"}]}));
    assert_eq!(
        written(&body, VALUES, Some(SpecVersion::new(1, 2)), None)["says"],
        json!({"type": "Property", "values": [[{"en": "yes"}, "2025-01-01T00:00:00Z"]]})
    );
}

/// Clause 4.21: a `pick` that does not list `type` omits it.
#[test]
fn pick_without_type_omits_the_type() {
    let body = evolution(json!({"speed": [{"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"}]}));
    let pick = [ProjectionTerm::Id];
    let projection = StructuralProjection::new(&pick, &[]);
    let result = written_with(&TemporalRoot::new(&body, Some(projection)), NORMALIZED, None, None);
    assert_eq!(result["id"], json!("urn:ngsi-ld:Vehicle:1"));
    assert!(result.get("type").is_none(), "{result}");
}

/// Clause 5.7.3.5: a projection that leaves no Entity member is no evolution.
#[test]
fn projection_leaving_nothing_is_not_found() {
    let body = TemporalBody::Evolution(match evolution(json!({})) {
        TemporalBody::Evolution(entity) => entity,
        TemporalBody::Aggregated(_) => panic!("an evolution is built"),
    });
    let pick = [ProjectionTerm::Scope];
    let projection = StructuralProjection::new(&pick, &[]);
    assert!(!TemporalRoot::new(&body, Some(projection)).carries_members(NORMALIZED));
    assert!(TemporalRoot::new(&body, None).carries_members(NORMALIZED));
}

const WEIGHT: &str = "https://uri.etsi.org/ngsi-ld/default-context/weight";

fn instant(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
}

/// One period of one attribute instance, with every aggregate absent.
fn bucket(bucket_start: Option<&str>, bucket_end: Option<&str>) -> TemporalAggregationRow {
    TemporalAggregationRow {
        attribute_name: WEIGHT.parse().expect("valid attribute name"),
        attribute_type: AttributeType::Property,
        dataset_id: None,
        bucket_start: bucket_start.map(instant),
        bucket_end: bucket_end.map(instant),
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

/// The aggregated instance the rows write for the weight attribute.
fn aggregated(rows: &[TemporalAggregationRow], methods: &[AggrMethod], depth: usize) -> Value {
    let entity = AggregatedTemporalEntity::from_rows(
        "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
        EntityTypeValue::Single("Vehicle".parse().expect("a type")),
        rows,
        methods,
        depth,
        AttributeName::clone,
    );
    let body = TemporalBody::Aggregated(entity);
    let format = if depth == 0 {
        TemporalFormat::AggregatedValues
    } else {
        TemporalFormat::GroupedValues
    };
    written(&body, representation(format, TimeProp::Observed), None, None)[WEIGHT].clone()
}

/// Clause 4.5.19: the method member holds one `[value, start, end]` triple per
/// period.
#[test]
fn aggregated_values_write_method_triples() {
    let mut row = bucket(Some("2026-01-01T00:00:00Z"), Some("2026-01-01T01:00:00Z"));
    row.avg = Some(AggregationAverage::Number(JsonNumber::from_str("2.5").expect("a number")));
    assert_eq!(
        aggregated(&[row], &[AggrMethod::Avg], 0),
        json!({"type": "Property", "avg": [[2.5, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z"]]})
    );
}

/// An exact integer beyond 2^53 is written as itself.
#[test]
fn an_exact_sum_is_written_as_the_integer_it_is() {
    let mut row = bucket(None, None);
    row.sum = Some(JsonNumber::from(9_007_199_254_740_993_i64));
    let instance = aggregated(&[row], &[AggrMethod::Sum], 0);
    assert_eq!(instance["sum"], json!([[9_007_199_254_740_993_i64, Value::Null, Value::Null]]));
    assert!(serde_json::to_string(&instance).expect("serialises").contains("9007199254740993"), "{instance}");
}

#[test]
fn every_decimal_aggregate_is_written_without_binary64_conversion() {
    let exact = |value: &str| JsonNumber::from_str(value).expect("supported exact aggregate");
    let mut row = bucket(None, None);
    row.avg = Some(AggregationAverage::Number(exact("0.150000000000000005")));
    row.min = Some(AggregationMinMax::Number(exact("0.100000000000000005")));
    row.max = Some(AggregationMinMax::Number(exact("0.200000000000000005")));
    row.sum = Some(exact("0.300000000000000010"));
    row.sumsq = Some(exact("0.050000000000000003000000000000000050"));
    row.stddev = Some(exact("0.070710678118654752"));
    let instance = aggregated(
        &[row],
        &[
            AggrMethod::Avg,
            AggrMethod::Min,
            AggrMethod::Max,
            AggrMethod::Sum,
            AggrMethod::SumSq,
            AggrMethod::StdDev,
        ],
        0,
    );
    let encoded = serde_json::to_string(&instance).expect("aggregate JSON");
    for expected in [
        "0.150000000000000005",
        "0.100000000000000005",
        "0.200000000000000005",
        "0.30000000000000001",
        "0.05000000000000000300000000000000005",
        "0.070710678118654752",
    ] {
        assert!(encoded.contains(expected), "missing {expected} in {encoded}");
    }
}

/// Every extremum family is written in its own JSON shape, and an instant with
/// the sub-second precision it was observed with.
#[test]
fn each_extremum_family_is_written_in_its_own_json_shape() {
    let cases = [
        (AggregationMinMax::Number(JsonNumber::try_from(1.5).expect("a finite number")), json!(1.5)),
        (AggregationMinMax::String("alpha".to_owned()), json!("alpha")),
        (AggregationMinMax::Boolean(false), json!(false)),
        (
            AggregationMinMax::DateTime(instant("2026-01-01T08:39:59.810Z")),
            json!("2026-01-01T08:39:59.810Z"),
        ),
    ];
    for (extremum, expected) in cases {
        let mut row = bucket(None, None);
        row.min = Some(extremum.clone()); // clone: the case table is reused for the failure message
        let instance = aggregated(&[row], &[AggrMethod::Min], 0);
        assert_eq!(instance["min"], json!([[expected, Value::Null, Value::Null]]), "{extremum:?}");
    }
}

/// A method the row carries no value for is a null member of its triple.
#[test]
fn an_absent_aggregate_is_written_as_a_null_member_of_its_triple() {
    let instance = aggregated(&[bucket(Some("2026-01-01T00:00:00Z"), Some("2026-01-01T01:00:00Z"))], &[AggrMethod::Avg], 0);
    assert_eq!(instance["avg"], json!([[Value::Null, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z"]]));
}

/// Bucket bounds derived from a `timeAt` carrying milliseconds are written
/// with them.
#[test]
fn bucket_bounds_keep_the_sub_second_precision_of_the_window() {
    let instance = aggregated(
        &[bucket(Some("2026-01-01T00:00:00.050Z"), Some("2026-01-01T00:00:00.200Z"))],
        &[AggrMethod::TotalCount],
        0,
    );
    assert_eq!(
        instance["totalCount"],
        json!([[Value::Null, "2026-01-01T00:00:00.050Z", "2026-01-01T00:00:00.200Z"]])
    );
}

/// Clause 4.5.4: two instances told apart by `datasetId` are an array of two
/// objects.
#[test]
fn two_dataset_instances_are_written_as_an_array_of_instances() {
    let mut default_instance = bucket(None, None);
    default_instance.total_count = Some(1);
    let mut named_instance = bucket(None, None);
    named_instance.dataset_id = Some("urn:ngsi-ld:Dataset:one".parse().expect("valid dataset id"));
    named_instance.total_count = Some(4);
    let instance = aggregated(&[default_instance, named_instance], &[AggrMethod::TotalCount], 0);
    let instances = instance.as_array().expect("two instances are an array");
    assert_eq!(instances.len(), 2, "{instance}");
    assert_eq!(instances[1]["datasetId"], json!("urn:ngsi-ld:Dataset:one"));
}

fn grouped_bucket(count: i64, keys: Vec<GroupKeyValue>) -> TemporalAggregationRow {
    let mut row = bucket(None, None);
    row.total_count = Some(count);
    row.group_keys = keys;
    row
}

/// Study section 4.6: grouping terms nest a map keyed by the group value
/// between the method member and its triples, absent as `@none` and Null as
/// `urn:ngsi-ld:null`.
#[test]
fn grouping_terms_nest_maps_keyed_by_the_group_value() {
    let by_unit = aggregated(
        &[
            grouped_bucket(2, vec![GroupKeyValue::Value("KGM".to_owned())]),
            grouped_bucket(3, vec![GroupKeyValue::Value("LBR".to_owned())]),
        ],
        &[AggrMethod::TotalCount],
        1,
    );
    assert_eq!(by_unit["totalCount"]["KGM"], json!([[2, Value::Null, Value::Null]]));
    assert_eq!(by_unit["totalCount"]["LBR"], json!([[3, Value::Null, Value::Null]]));

    let by_day = aggregated(
        &[grouped_bucket(
            5,
            vec![GroupKeyValue::Value("KGM".to_owned()), GroupKeyValue::Value("2026-01-01T00:00:00Z".to_owned())],
        )],
        &[AggrMethod::TotalCount],
        2,
    );
    assert_eq!(by_day["totalCount"]["KGM"]["2026-01-01T00:00:00Z"], json!([[5, Value::Null, Value::Null]]));

    let reserved = aggregated(
        &[grouped_bucket(1, vec![GroupKeyValue::Absent]), grouped_bucket(2, vec![GroupKeyValue::Null])],
        &[AggrMethod::TotalCount],
        1,
    );
    assert_eq!(reserved["totalCount"]["@none"], json!([[1, Value::Null, Value::Null]]));
    assert_eq!(reserved["totalCount"]["urn:ngsi-ld:null"], json!([[2, Value::Null, Value::Null]]));
}

/// Clause 4.3.6.8 over an aggregated entity: below 1.3 the `datasetId` set
/// collapses to its default instance.
#[test]
fn an_aggregated_entity_collapses_its_dataset_set_below_1_3() {
    let mut default_instance = bucket(None, None);
    default_instance.total_count = Some(1);
    let mut named_instance = bucket(None, None);
    named_instance.dataset_id = Some("urn:ngsi-ld:Dataset:one".parse().expect("valid dataset id"));
    named_instance.total_count = Some(4);
    let entity = AggregatedTemporalEntity::from_rows(
        "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
        EntityTypeValue::Single("Vehicle".parse().expect("a type")),
        &[default_instance, named_instance],
        &[AggrMethod::TotalCount],
        0,
        AttributeName::clone,
    );
    let body = TemporalBody::Aggregated(entity);
    let result = written(
        &body,
        representation(TemporalFormat::AggregatedValues, TimeProp::Observed),
        Some(SpecVersion::V1_0),
        None,
    );
    assert!(result[WEIGHT].is_object(), "{result}");
    assert!(result[WEIGHT].get("datasetId").is_none(), "{result}");
}

/// The raw text `body` is written as with an embedded `@context`.
fn embedded_text(body: &TemporalBody, representation: TemporalRepresentation) -> String {
    let context = JsonLiteral::admit(r#""https://example.org/ctx.jsonld""#).expect("an admissible literal");
    let mut bytes = Vec::new();
    ResponseRenderer::new(ResponseMediaType::JsonLd, ResponseContext::Body(context))
        .with_temporal(TemporalPayload::Single(TemporalRoot::new(body, None)), representation, None)
        .render_into(&mut bytes)
        .expect("the evolution is written");
    String::from_utf8(bytes).expect("a JSON body is UTF-8")
}

/// Clause 6.3.6: `@context` is the first member of a Temporal Evolution in
/// the normalized, simplified and aggregated representations.
#[test]
fn a_response_context_member_comes_first_in_every_temporal_body() {
    let evolution = evolution(json!({"speed": {"type": "Property", "value": 25, "observedAt": "2025-01-01T00:00:00Z"}}));
    for representation in [NORMALIZED, VALUES] {
        let text = embedded_text(&evolution, representation);
        assert!(text.starts_with(r#"{"@context":"https://example.org/ctx.jsonld","#), "{text}");
    }
    let mut row = bucket(None, None);
    row.total_count = Some(1);
    let aggregate = TemporalBody::Aggregated(AggregatedTemporalEntity::from_rows(
        "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
        EntityTypeValue::Single("Vehicle".parse().expect("a type")),
        &[row],
        &[AggrMethod::TotalCount],
        0,
        AttributeName::clone,
    ));
    let text = embedded_text(&aggregate, representation(TemporalFormat::AggregatedValues, TimeProp::Observed));
    assert!(text.starts_with(r#"{"@context":"https://example.org/ctx.jsonld","#), "{text}");
}
