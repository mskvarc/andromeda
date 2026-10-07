//! Which aggregation method each value datatype admits, and what it computes.
//!
//! Tables 4.5.19.1-1, 4.5.19.1-2 and 4.5.19.1-3 of ETSI GS CIM 009 v1.9.1 fix
//! one column per value datatype and one row per method, and clause 5.7.4.4
//! makes a request naming a method its column marks N/A an `InvalidRequest`.
//! Two columns are not a neighbouring column: a JSON Array is not folded into
//! the JSON Object beside it, which would lose the four methods the table
//! defines over array sizes, and a `Time` is not folded into the `DateTime`
//! beside it, which would lose the `avg` the table defines over times of day.
//!
//! Every test names the table and the column it transcribes, and asserts the
//! value that table requires.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The two observation instants every fixture in this module records at, one
/// hour apart, so an unbucketed request folds both into a single period.
const FIRST_OBSERVATION: &str = "2024-01-01T00:00:00Z";
/// The second of the two observation instants.
const SECOND_OBSERVATION: &str = "2024-01-01T01:00:00Z";

/// A temporal window wide enough to cover both observations.
const WHOLE_WINDOW: &str = "timerel=after&timeAt=2023-12-31T00:00:00Z";

/// One temporal entity carrying the given instances of one attribute.
fn entity_with(id: &str, attribute: &str, instances: &[Value]) -> Value {
    json!({
        "id": id,
        "type": "AggregationSubject",
        attribute: instances,
        "@context": CORE_CONTEXT
    })
}

/// One Property instance whose value is a JSON Array of `size` elements.
///
/// The array is carried as a JSON literal so that JSON-LD expansion keeps it
/// one array value; an untyped term would expand `[1, 2]` into two separate
/// values of the Property instead, which is a different attribute shape
/// entirely and not the one Table 4.5.19.1-1's "JSON Array" column names.
fn array_instance(size: usize, observed_at: &str) -> Value {
    let elements: Vec<Value> = (1..=size).map(|element| json!(element)).collect();
    json!({
        "type": "Property",
        "value": {"@type": "@json", "@value": elements},
        "observedAt": observed_at
    })
}

/// One Property instance whose value carries the clause 4.6.3 type `kind`.
fn typed_instance(kind: &str, value: &str, observed_at: &str) -> Value {
    json!({
        "type": "Property",
        "value": {"@type": kind, "@value": value},
        "observedAt": observed_at
    })
}

/// Read an aggregated temporal representation of one attribute.
///
/// `attrs` is always named: clause 5.7.4.4 checks eligibility against the
/// attributes the request addresses, and a request that addresses all of them
/// has none to check ahead of the read.
async fn aggregated(client: &TestClient, id: &str, attribute: &str, methods: &str) -> TestResponse {
    let query = format!("{WHOLE_WINDOW}&attrs={attribute}&format=aggregatedValues&aggrMethods={methods}");
    client
        .get_temporal_entity(id, &query, Some(CORE_CONTEXT))
        .await
        .expect("aggregated temporal retrieve failed")
}

/// The single aggregate value one method produced for one attribute.
///
/// Clause 4.5.19 renders each method as a list of `[value, start, end]`
/// triples, one per period; an unbucketed request has exactly one.
fn only_aggregate<'body>(body: &'body Value, attribute: &str, method: &str) -> &'body Value {
    let triples = body[attribute][method].as_array().expect("an aggregate renders as a list of triples");
    assert_eq!(triples.len(), 1, "an unbucketed request covers one period: {body}");
    &triples[0].as_array().expect("each period renders as a triple")[0]
}

/// Assert that one aggregate is the whole number the table requires.
///
/// The broker renders an aggregate as an arbitrary-precision JSON number, so an
/// average of exactly three arrives spelled with its `NUMERIC` scale; the
/// comparison is written with a tolerance because a strict float equality is
/// not, while every expected value here is exact in binary64.
fn assert_whole_aggregate(value: &Value, expected: f64, context: &str) {
    let actual = value.as_f64().expect("an array-size aggregate is a number");
    assert!((actual - expected).abs() < f64::EPSILON, "{context}: expected {expected}, got {actual}");
}

/// Assert that one method over one attribute is refused as an `InvalidRequest`.
async fn assert_refused(client: &TestClient, id: &str, attribute: &str, method: &str) {
    let response = aggregated(client, id, attribute, method).await;
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "{method} over {attribute} is N/A and must be refused: {:?}",
        response.body
    );
}

/// Assert that one method over one attribute is answered.
async fn assert_answered(client: &TestClient, id: &str, attribute: &str, method: &str) -> Value {
    let response = aggregated(client, id, attribute, method).await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "{method} over {attribute} is defined and must be answered: {:?}",
        response.body
    );
    response.json_value().expect("an aggregated representation must be valid JSON")
}

/// Table 4.5.19.1-1, "JSON Array" column, `avg` row: "Calculate the average
/// number of the sizes of the arrays inside the period". Arrays of two and of
/// four elements average to three. The JSON Object column's `avg` cell is N/A,
/// so a JSON Array must not be folded into it.
#[tokio::test]
async fn regression_table_4_5_19_1_1_avg_over_json_arrays_is_the_average_array_size() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:array-avg");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "tags",
            &[array_instance(2, FIRST_OBSERVATION), array_instance(4, SECOND_OBSERVATION)],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let body = assert_answered(client, &id, "tags", "avg").await;
    assert_whole_aggregate(
        only_aggregate(&body, "tags", "avg"),
        3.0,
        &format!("arrays of size 2 and 4 average to 3: {body}"),
    );

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-1, "JSON Array" column, `min`, `max` and `sum` rows: the
/// minimum size, the maximum size, and "the sum of the sizes of arrays inside
/// the period". Over arrays of two and four elements that is 2, 4 and 6.
#[tokio::test]
async fn regression_table_4_5_19_1_1_min_max_and_sum_over_json_arrays_measure_array_size() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:array-extrema");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "tags",
            &[array_instance(2, FIRST_OBSERVATION), array_instance(4, SECOND_OBSERVATION)],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let body = assert_answered(client, &id, "tags", "min,max,sum").await;
    for (method, expected) in [("min", 2.0), ("max", 4.0), ("sum", 6.0)] {
        assert_whole_aggregate(
            only_aggregate(&body, "tags", method),
            expected,
            &format!("{method} over arrays of size 2 and 4: {body}"),
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-1, "JSON Array" column, `stddev` and `sumsq` rows: both are
/// N/A, so clause 5.7.4.4 makes a request naming either an `InvalidRequest`.
/// They are the only two of the eight the array column refuses, which separates
/// it from the JSON Object column.
#[tokio::test]
async fn regression_table_4_5_19_1_1_stddev_and_sumsq_over_json_arrays_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:array-na");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "tags",
            &[array_instance(2, FIRST_OBSERVATION), array_instance(4, SECOND_OBSERVATION)],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for method in ["stddev", "sumsq"] {
        assert_refused(client, &id, "tags", method).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-2, "Time" column, `avg` row: "Calculate the average time
/// inside the period", the clause's own example being the time a car enters a
/// parking. Entries at 08:00 and 10:00 UTC average to 09:00 UTC. A `Time` is not
/// aggregated as the `DateTime` beside it, whose `avg` cell is N/A.
#[tokio::test]
async fn regression_table_4_5_19_1_2_avg_over_a_time_property_is_the_average_time_of_day() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:time-avg");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "entryTime",
            &[
                typed_instance("Time", "08:00:00Z", FIRST_OBSERVATION),
                typed_instance("Time", "10:00:00Z", SECOND_OBSERVATION),
            ],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let body = assert_answered(client, &id, "entryTime", "avg").await;
    let avg = only_aggregate(&body, "entryTime", "avg");
    let rendered = avg.as_str().expect("the average of two times of day is a Time value");
    assert!(
        rendered.starts_with("09:00:00"),
        "08:00 and 10:00 average to 09:00 (Table 4.5.19.1-2, Time column): {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-2, "Date" and "`DateTime`" columns, `avg` row: both are N/A,
/// unlike the "Time" column between them. Averaging a calendar day or an
/// instant is not defined, and the broker must refuse both although it averages
/// a time of day.
#[tokio::test]
async fn regression_table_4_5_19_1_2_avg_over_a_date_or_a_date_time_property_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:temporal-na");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "AggregationSubject",
            "entryDate": [
                typed_instance("Date", "2024-03-01", FIRST_OBSERVATION),
                typed_instance("Date", "2024-03-03", SECOND_OBSERVATION),
            ],
            "seenAt": [
                typed_instance("DateTime", "2024-03-01T05:00:00Z", FIRST_OBSERVATION),
                typed_instance("DateTime", "2024-03-01T07:00:00Z", SECOND_OBSERVATION),
            ],
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for attribute in ["entryDate", "seenAt"] {
        assert_refused(client, &id, attribute, "avg").await;
        // The same two columns give `min` and `max`, so the refusal above must
        // be the `avg` cell rather than the whole column being unreachable.
        let body = assert_answered(client, &id, attribute, "min,max").await;
        assert!(only_aggregate(&body, attribute, "min").is_string(), "{body}");
        assert!(only_aggregate(&body, attribute, "max").is_string(), "{body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The `distinctCount` and `totalCount` rows of Tables 4.5.19.1-1, 4.5.19.1-2
/// and 4.5.19.1-3 span every column with no N/A cell: both count instances
/// rather than reading values, so every value datatype admits them. Two
/// distinct observations of each family give a count of two on both rows.
#[tokio::test]
async fn regression_every_value_datatype_admits_both_counting_methods() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:counting");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "AggregationSubject",
            "speed": [
                {"type": "Property", "value": 10, "observedAt": FIRST_OBSERVATION},
                {"type": "Property", "value": 20, "observedAt": SECOND_OBSERVATION},
            ],
            "brandName": [
                {"type": "Property", "value": "alpha", "observedAt": FIRST_OBSERVATION},
                {"type": "Property", "value": "beta", "observedAt": SECOND_OBSERVATION},
            ],
            "isParked": [
                {"type": "Property", "value": true, "observedAt": FIRST_OBSERVATION},
                {"type": "Property", "value": false, "observedAt": SECOND_OBSERVATION},
            ],
            "settings": [
                {"type": "JsonProperty", "json": {"mode": "eco"}, "observedAt": FIRST_OBSERVATION},
                {"type": "JsonProperty", "json": {"mode": "sport"}, "observedAt": SECOND_OBSERVATION},
            ],
            "tags": [array_instance(2, FIRST_OBSERVATION), array_instance(4, SECOND_OBSERVATION)],
            "seenAt": [
                typed_instance("DateTime", "2024-03-01T05:00:00Z", FIRST_OBSERVATION),
                typed_instance("DateTime", "2024-03-01T07:00:00Z", SECOND_OBSERVATION),
            ],
            "entryDate": [
                typed_instance("Date", "2024-03-01", FIRST_OBSERVATION),
                typed_instance("Date", "2024-03-03", SECOND_OBSERVATION),
            ],
            "entryTime": [
                typed_instance("Time", "08:00:00Z", FIRST_OBSERVATION),
                typed_instance("Time", "10:00:00Z", SECOND_OBSERVATION),
            ],
            "parkedAt": [
                {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:1", "observedAt": FIRST_OBSERVATION},
                {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:2", "observedAt": SECOND_OBSERVATION},
            ],
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for attribute in [
        "speed",
        "brandName",
        "isParked",
        "settings",
        "tags",
        "seenAt",
        "entryDate",
        "entryTime",
        "parkedAt",
    ] {
        let body = assert_answered(client, &id, attribute, "distinctCount,totalCount").await;
        for method in ["distinctCount", "totalCount"] {
            assert_eq!(
                only_aggregate(&body, attribute, method).as_i64().expect("a count is an integer"),
                2,
                "{method} over two distinct observations of {attribute} is 2: {body}"
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-3: a Relationship has `distinctCount` over its targets and
/// `totalCount` over its updates, and every other method is N/A. Nothing about
/// a set of target URIs has an average, an extremum or a scale.
#[tokio::test]
async fn regression_table_4_5_19_1_3_a_relationship_admits_only_the_counting_methods() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:relationship");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "parkedAt",
            &[
                json!({"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:1", "observedAt": FIRST_OBSERVATION}),
                json!({"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:2", "observedAt": SECOND_OBSERVATION}),
            ],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for method in ["avg", "min", "max", "stddev", "sum", "sumsq"] {
        assert_refused(client, &id, "parkedAt", method).await;
    }
    let body = assert_answered(client, &id, "parkedAt", "distinctCount,totalCount").await;
    assert_eq!(only_aggregate(&body, "parkedAt", "distinctCount").as_i64().expect("a count"), 2, "{body}");
    assert_eq!(only_aggregate(&body, "parkedAt", "totalCount").as_i64().expect("a count"), 2, "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-1, "JSON Object" column: every method but the two counting
/// ones is N/A, which is exactly what the JSON Array column beside it is not.
/// The two columns share one storage representation, so this proves only the
/// array column admits the array methods.
#[tokio::test]
async fn regression_table_4_5_19_1_1_a_json_object_still_admits_counting_alone() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:object");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "settings",
            &[
                json!({"type": "JsonProperty", "json": {"mode": "eco"}, "observedAt": FIRST_OBSERVATION}),
                json!({"type": "JsonProperty", "json": {"mode": "sport"}, "observedAt": SECOND_OBSERVATION}),
            ],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for method in ["avg", "min", "max", "stddev", "sum", "sumsq"] {
        assert_refused(client, &id, "settings", method).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 4.5.19.1-1, "JSON String" column: `min` and `max` are the first and
/// last value in lexicographical order and the arithmetic methods are N/A. A
/// string is stored in the same column as a `Time` and a `Date`, so it must not
/// get the `avg` the `Time` column defines.
#[tokio::test]
async fn regression_table_4_5_19_1_1_a_json_string_admits_ordering_but_no_arithmetic() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:string");

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "brandName",
            &[
                json!({"type": "Property", "value": "beta", "observedAt": FIRST_OBSERVATION}),
                json!({"type": "Property", "value": "alpha", "observedAt": SECOND_OBSERVATION}),
            ],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for method in ["avg", "stddev", "sum", "sumsq"] {
        assert_refused(client, &id, "brandName", method).await;
    }
    let body = assert_answered(client, &id, "brandName", "min,max").await;
    assert_eq!(
        only_aggregate(&body, "brandName", "min").as_str().expect("a string extremum"),
        "alpha",
        "{body}"
    );
    assert_eq!(only_aggregate(&body, "brandName", "max").as_str().expect("a string extremum"), "beta", "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// One `ListRelationship` instance whose `objectList` names `targets`.
fn list_relationship_instance(targets: &[&str], observed_at: &str) -> Value {
    json!({
        "type": "ListRelationship",
        "objectList": targets,
        "observedAt": observed_at
    })
}

/// One `VocabProperty` instance naming several vocabulary members at once.
///
/// Clause 4.5.20.2: `vocab` is "a single string or array of strings which can
/// be type coerced into an IRI or array of IRIs".
fn multi_vocab_instance(members: &[&str], observed_at: &str) -> Value {
    json!({
        "type": "VocabProperty",
        "vocab": members,
        "observedAt": observed_at
    })
}

/// One Relationship instance pointing at every URI in `targets`.
///
/// Clause 4.5.3.2: `object` is "the Relationship's object represented by a URI
/// or array of URIs".
fn relationship_instance(targets: &[&str], observed_at: &str) -> Value {
    json!({
        "type": "Relationship",
        "object": targets,
        "observedAt": observed_at
    })
}

/// Read an aggregated temporal representation over every attribute.
///
/// Clause 5.7.4.4's eligibility check runs against the attributes the request
/// addresses, so a request that names none is not screened ahead of the read at
/// all; what it answers is decided by the statement alone.
async fn aggregated_without_attrs(client: &TestClient, id: &str, methods: &str) -> Value {
    let query = format!("{WHOLE_WINDOW}&format=aggregatedValues&aggrMethods={methods}");
    let response = client
        .get_temporal_entity(id, &query, Some(CORE_CONTEXT))
        .await
        .expect("aggregated temporal retrieve failed");
    assert_eq!(response.status, StatusCode::OK, "an unscreened aggregation still answers: {:?}", response.body);
    response.json_value().expect("an aggregated representation must be valid JSON")
}

/// The instances of one attribute in a normalized temporal representation,
/// ordered by the `observedAt` each carries.
fn instances_by_observation<'body>(body: &'body Value, attribute: &str) -> Vec<&'body Value> {
    assert!(body[attribute].is_array(), "attribute {attribute} renders as an array of instances: {body}");
    let mut instances: Vec<&Value> = body[attribute].as_array().expect("an array of instances").iter().collect();
    instances.sort_by_key(|instance| instance["observedAt"].as_str().unwrap_or_default().to_owned());
    instances
}

/// Table 4.5.19.1-3, "Relationship" column: `avg`, `max`, `min`, `stddev`,
/// `sum` and `sumsq` are all N/A, and only `distinctCount` ("the count of
/// distinct relationships targets inside the period") and `totalCount` are
/// defined. Clause 4.5.22.1 makes `ListRelationship` "a specialized type of
/// Relationship", so that single column governs it too — its `objectList` is
/// an array of targets, not a JSON Array whose sizes Table 4.5.19.1-1 averages.
/// Table 4.5.19.1-2's "URI" column says the same of a `VocabProperty`, whose
/// `vocab` clause 4.5.20.2 defines as "a single string or array of strings
/// which can be type coerced into an IRI or array of IRIs".
///
/// The two attributes carry lists of two and of four members, so a broker that
/// aggregated them by array size would answer `avg` with 3 rather than refusing.
#[tokio::test]
async fn regression_table_4_5_19_1_3_a_list_relationship_and_a_vocab_property_admit_counting_alone() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:relationship-family");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "AggregationSubject",
            "route": [
                list_relationship_instance(&["urn:ngsi-ld:City:Antwerp", "urn:ngsi-ld:City:Rotterdam"], FIRST_OBSERVATION),
                list_relationship_instance(
                    &["urn:ngsi-ld:City:Antwerp", "urn:ngsi-ld:City:Rotterdam", "urn:ngsi-ld:City:Amsterdam", "urn:ngsi-ld:City:Ghent"],
                    SECOND_OBSERVATION
                ),
            ],
            "categories": [
                multi_vocab_instance(&["alpha", "beta"], FIRST_OBSERVATION),
                multi_vocab_instance(&["alpha", "beta", "gamma", "delta"], SECOND_OBSERVATION),
            ],
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for attribute in ["route", "categories"] {
        for method in ["avg", "min", "max", "stddev", "sum", "sumsq"] {
            assert_refused(client, &id, attribute, method).await;
        }
        let body = assert_answered(client, &id, attribute, "distinctCount,totalCount").await;
        assert_eq!(
            only_aggregate(&body, attribute, "totalCount").as_i64().expect("a count is an integer"),
            2,
            "totalCount counts the two updates of {attribute}: {body}"
        );
    }

    // Clause 5.7.4.4's check only screens the attributes a request names, so a
    // request naming none reaches the statement unscreened. The refusal has to
    // hold there too: no arithmetic aggregate may be computed for a family
    // Table 4.5.19.1-3 marks N/A, whatever shape the targets are stored in.
    let body = aggregated_without_attrs(client, &id, "avg,min,max,sum,sumsq,stddev").await;
    for attribute in ["route", "categories"] {
        for method in ["avg", "min", "max", "sum", "sumsq", "stddev"] {
            assert_eq!(
                only_aggregate(&body, attribute, method),
                &Value::Null,
                "{method} over {attribute} is N/A and must yield no value at all: {body}"
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.3.2, Mandatory: "`object`: the Relationship's object represented
/// by a URI **or array of URIs**". Clause 4.5.9 EXAMPLE 7 shows exactly that in
/// a temporal evolution — an `activeDevices` Relationship whose `objects` pairs
/// hold two targets at one instant and three at the next — so every target has
/// to survive into the Temporal Evolution rather than only the first.
#[tokio::test]
async fn regression_clause_4_5_3_2_a_multi_target_relationship_keeps_every_target() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:multi-target");
    let first = ["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562"];
    let second = ["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562", "urn:ngsi-ld:Device:37309"];

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "activeDevices",
            &[
                relationship_instance(&first, FIRST_OBSERVATION),
                relationship_instance(&second, SECOND_OBSERVATION),
            ],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let response = client
        .get_temporal_entity(&id, WHOLE_WINDOW, Some(CORE_CONTEXT))
        .await
        .expect("temporal retrieve failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve: {:?}", response.body);
    let body = response.json_value().expect("a temporal representation must be valid JSON");

    let instances = instances_by_observation(&body, "activeDevices");
    assert_eq!(instances.len(), 2, "two observations were recorded: {body}");
    assert_eq!(instances[0]["object"], json!(first), "clause 4.5.3.2 keeps every target: {body}");
    assert_eq!(instances[1]["object"], json!(second), "clause 4.5.3.2 keeps every target: {body}");

    // Clause 4.5.9, Relationship bullet: "the first element shall be a
    // Relationship object (a URI or array of URIs)", which EXAMPLE 7 spells as
    // the array of targets the observation named.
    let simplified = client
        .get_temporal_entity(&id, &format!("{WHOLE_WINDOW}&options=temporalValues"), Some(CORE_CONTEXT))
        .await
        .expect("simplified temporal retrieve failed");
    assert_eq!(simplified.status, StatusCode::OK, "temporalValues: {:?}", simplified.body);
    let simplified = simplified.json_value().expect("a simplified temporal representation must be valid JSON");
    assert!(
        simplified["activeDevices"]["objects"].is_array(),
        "clause 4.5.9 collapses a Relationship into `objects`: {simplified}"
    );
    let mut pairs = simplified["activeDevices"]["objects"].as_array().expect("the clause 4.5.9 pairs").clone();
    pairs.sort_by_key(|pair| pair[1].as_str().unwrap_or_default().to_owned());
    assert_eq!(pairs[0][0], json!(first), "clause 4.5.9 EXAMPLE 7 pairs an array of URIs: {simplified}");
    assert_eq!(pairs[1][0], json!(second), "clause 4.5.9 EXAMPLE 7 pairs an array of URIs: {simplified}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2, Mandatory: "`objectList`: a JSON representation of an
/// ordered array of Relationship Objects each consisting of a JSON object
/// containing a single Attribute with a key called `object` and its value
/// holding the Relationship's object represented by a URI." Table 5.2.37-1
/// splits the two representations explicitly — "In the normalized form, each
/// array element holds a JSON object containing a single Attribute with a key
/// called `object` ... In the concise form, each string in the array holds a
/// valid URI" — and a temporal retrieval answers in the normalized form.
///
/// The same clause makes that normalized form a legal thing for a Context
/// Producer to write, so it round-trips: what a temporal write accepts and what
/// a temporal read answers are the same shape.
#[tokio::test]
async fn regression_clause_4_5_22_2_a_normalized_object_list_renders_object_keyed_members() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("AggregationSubject:object-list-shape");
    let targets = ["urn:ngsi-ld:City:Antwerp", "urn:ngsi-ld:City:Rotterdam"];
    let normalized = json!([{"object": targets[0]}, {"object": targets[1]}]);

    let created = client
        .create_temporal_entity(&entity_with(
            &id,
            "route",
            &[json!({
                "type": "ListRelationship",
                "objectList": normalized,
                "observedAt": FIRST_OBSERVATION
            })],
        ))
        .await
        .expect("create temporal entity failed");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "clause 4.5.22.2's normalized objectList is a conformant write: {:?}",
        created.body
    );

    let response = client
        .get_temporal_entity(&id, WHOLE_WINDOW, Some(CORE_CONTEXT))
        .await
        .expect("temporal retrieve failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve: {:?}", response.body);
    let body = response.json_value().expect("a temporal representation must be valid JSON");
    let instances = instances_by_observation(&body, "route");
    assert_eq!(instances.len(), 1, "one observation was recorded: {body}");
    assert_eq!(
        instances[0]["objectList"], normalized,
        "Table 5.2.37-1: in the normalized form each element holds a JSON object keyed `object`: {body}"
    );

    // Clause 4.5.9, `ListRelationship` bullet: the simplified temporal
    // representation's `objectLists` pairs hold "an ordered array of
    // Relationship objects (URIs)", which EXAMPLE 7 spells as bare strings.
    let simplified = client
        .get_temporal_entity(&id, &format!("{WHOLE_WINDOW}&options=temporalValues"), Some(CORE_CONTEXT))
        .await
        .expect("simplified temporal retrieve failed");
    assert_eq!(simplified.status, StatusCode::OK, "temporalValues: {:?}", simplified.body);
    let simplified = simplified.json_value().expect("a simplified temporal representation must be valid JSON");
    assert_eq!(
        simplified["route"]["objectLists"][0][0],
        json!(targets),
        "clause 4.5.9 pairs an ordered array of URIs: {simplified}"
    );

    // Table 5.2.37-1, concise form: "each string in the array holds a valid
    // URI". The concise representation is answered by the current-state
    // endpoint, so the same list is written there and read back concisely.
    let concise_id = scope.id("AggregationSubject:object-list-concise");
    let created = client
        .post_entity(&json!({
            "id": concise_id,
            "type": "AggregationSubject",
            "route": {"type": "ListRelationship", "objectList": normalized},
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let concise = client
        .get_entity(&concise_id, "options=concise", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("concise retrieve failed");
    assert_eq!(concise.status, StatusCode::OK, "concise: {:?}", concise.body);
    let concise = concise.json_value().expect("a concise representation must be valid JSON");
    assert_eq!(
        concise["route"]["objectList"],
        json!(targets),
        "Table 5.2.37-1: in the concise form each string in the array holds a valid URI: {concise}"
    );

    scope.finish().await.expect("cleanup failed");
}
