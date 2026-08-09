#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Integration tests for NGSI-LD temporal aggregation (spec 4.5.19).
//!
//! Uses the standard Rust test harness with `#[tokio::test]` and raw `reqwest`.
//! Requires a running Andromeda instance on `127.0.0.1:1056` with a clean database.

use andromeda_test_common::config::{BASE_URL, CORE_CONTEXT};
use reqwest::Client;
use serde_json::{Value, json};
use uuid::Uuid;

const BASE: &iri_rs::Iri<&str> = BASE_URL;
const CTX: &iri_rs::Iri<&str> = CORE_CONTEXT;

fn uid(prefix: &str) -> String {
    format!("urn:ngsi-ld:AggTest:{}:{}", prefix, Uuid::new_v4())
}

async fn create(client: &Client, entity: &Value) {
    let resp = client
        .post(format!("{BASE}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create request failed");
    let status = resp.status();
    assert!(
        status == reqwest::StatusCode::CREATED || status == reqwest::StatusCode::NO_CONTENT,
        "create failed: {} — {}",
        status,
        resp.text().await.unwrap_or_default()
    );
}

async fn delete(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE}/temporal/entities/{}", urlencoding::encode(id))).send().await;
}

async fn get_aggregated(client: &Client, id: &str, methods: &str, period: Option<&str>) -> Value {
    let encoded_id = urlencoding::encode(id);
    let mut url = format!("{BASE}/temporal/entities/{encoded_id}?format=aggregatedValues&aggrMethods={methods}");
    if let Some(p) = period {
        url.push_str(&format!("&aggrPeriodDuration={p}"));
    }
    let resp = client
        .get(&url)
        .header("Accept", "application/ld+json")
        .header(
            "Link",
            format!("<{CTX}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .send()
        .await
        .expect("get request failed");
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    assert!(status.is_success(), "GET aggregated failed: {status} — {body}");
    serde_json::from_str(&body).expect("invalid JSON response")
}

async fn get_aggregated_raw(client: &Client, id: &str, query: &str) -> reqwest::Response {
    let encoded_id = urlencoding::encode(id);
    let url = format!("{BASE}/temporal/entities/{encoded_id}?{query}");
    client
        .get(&url)
        .header("Accept", "application/ld+json")
        .header(
            "Link",
            format!("<{CTX}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .send()
        .await
        .expect("request failed")
}

/// Build a temporal entity with a numeric Property having multiple observations.
fn numeric_entity(id: &str, attr: &str, values: &[(f64, &str)]) -> Value {
    let instances: Vec<Value> = values.iter().map(|(v, ts)| json!({"type": "Property", "value": v, "observedAt": ts})).collect();
    json!({
        "id": id,
        "type": "AggTestEntity",
        attr: instances,
        "@context": CTX
    })
}

/// Build a temporal entity with a boolean Property.
fn boolean_entity(id: &str, attr: &str, values: &[(bool, &str)]) -> Value {
    let instances: Vec<Value> = values.iter().map(|(v, ts)| json!({"type": "Property", "value": v, "observedAt": ts})).collect();
    json!({
        "id": id,
        "type": "AggTestEntity",
        attr: instances,
        "@context": CTX
    })
}

/// Build a temporal entity with a string Property.
fn string_entity(id: &str, attr: &str, values: &[(&str, &str)]) -> Value {
    let instances: Vec<Value> = values.iter().map(|(v, ts)| json!({"type": "Property", "value": v, "observedAt": ts})).collect();
    json!({
        "id": id,
        "type": "AggTestEntity",
        attr: instances,
        "@context": CTX
    })
}

/// Build a temporal entity with a Relationship.
fn relationship_entity(id: &str, attr: &str, values: &[(&str, &str)]) -> Value {
    let instances: Vec<Value> = values
        .iter()
        .map(|(obj, ts)| json!({"type": "Relationship", "object": obj, "observedAt": ts}))
        .collect();
    json!({
        "id": id,
        "type": "AggTestEntity",
        attr: instances,
        "@context": CTX
    })
}

// ---------------------------------------------------------------------------
// Validation tests (spec 4.5.19.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn aggregated_format_requires_aggr_methods() {
    let client = Client::new();
    let id = uid("val-001");
    let entity = numeric_entity(&id, "speed", &[(100.0, "2024-01-01T00:00:00Z")]);
    create(&client, &entity).await;

    let resp = get_aggregated_raw(&client, &id, "format=aggregatedValues").await;
    pretty_assertions::assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    delete(&client, &id).await;
}

#[tokio::test]
async fn invalid_aggr_method_returns_400() {
    let client = Client::new();
    let id = uid("val-002");
    let entity = numeric_entity(&id, "speed", &[(100.0, "2024-01-01T00:00:00Z")]);
    create(&client, &entity).await;

    let resp = get_aggregated_raw(&client, &id, "format=aggregatedValues&aggrMethods=bogus").await;
    pretty_assertions::assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Numeric Property aggregation (spec Table 4.5.19.1-1, JSON Number)
// ---------------------------------------------------------------------------

const NUMERIC_OBS: &[(f64, &str)] = &[
    (100.0, "2024-01-01T00:00:00Z"),
    (200.0, "2024-01-01T01:00:00Z"),
    (300.0, "2024-01-01T02:00:00Z"),
];

#[tokio::test]
async fn avg_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-avg");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    let avg_val = &body["speed"]["avg"][0][0];
    pretty_assertions::assert_eq!(avg_val.as_f64().expect("avg should be number"), 200.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn min_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-min");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "min", None).await;
    let val = &body["speed"]["min"][0][0];
    pretty_assertions::assert_eq!(val.as_f64().expect("min should be number"), 100.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn max_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-max");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "max", None).await;
    let val = &body["speed"]["max"][0][0];
    pretty_assertions::assert_eq!(val.as_f64().expect("max should be number"), 300.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn sum_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-sum");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "sum", None).await;
    let val = &body["speed"]["sum"][0][0];
    pretty_assertions::assert_eq!(val.as_f64().expect("sum should be number"), 600.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn sumsq_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-sumsq");
    // 10^2 + 20^2 = 100 + 400 = 500
    let obs = &[(10.0, "2024-01-01T00:00:00Z"), (20.0, "2024-01-01T01:00:00Z")];
    create(&client, &numeric_entity(&id, "speed", obs)).await;

    let body = get_aggregated(&client, &id, "sumsq", None).await;
    let val = &body["speed"]["sumsq"][0][0];
    pretty_assertions::assert_eq!(val.as_f64().expect("sumsq should be number"), 500.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn stddev_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-stddev");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "stddev", None).await;
    let val = body["speed"]["stddev"][0][0].as_f64().expect("stddev should be number");
    // stddev of [100, 200, 300] = 100.0 (sample stddev)
    assert!((val - 100.0).abs() < 0.01, "stddev should be ~100, got {val}");

    delete(&client, &id).await;
}

#[tokio::test]
async fn total_count_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-tc");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "totalCount", None).await;
    let val = &body["speed"]["totalCount"][0][0];
    pretty_assertions::assert_eq!(val.as_i64().expect("totalCount should be int"), 3);

    delete(&client, &id).await;
}

#[tokio::test]
async fn distinct_count_on_numeric_property() {
    let client = Client::new();
    let id = uid("num-dc");
    // [10, 20, 10] → 2 distinct
    let obs = &[(10.0, "2024-01-01T00:00:00Z"), (20.0, "2024-01-01T01:00:00Z"), (10.0, "2024-01-01T02:00:00Z")];
    create(&client, &numeric_entity(&id, "speed", obs)).await;

    let body = get_aggregated(&client, &id, "distinctCount", None).await;
    let val = &body["speed"]["distinctCount"][0][0];
    pretty_assertions::assert_eq!(val.as_i64().expect("distinctCount should be int"), 2);

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Boolean Property aggregation (spec Table 4.5.19.1-1, JSON Boolean)
// ---------------------------------------------------------------------------

const BOOL_OBS: &[(bool, &str)] = &[(true, "2024-01-01T00:00:00Z"), (false, "2024-01-01T01:00:00Z"), (true, "2024-01-01T02:00:00Z")];

#[tokio::test]
async fn avg_on_boolean_property() {
    let client = Client::new();
    let id = uid("bool-avg");
    create(&client, &boolean_entity(&id, "isActive", BOOL_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    let val = body["isActive"]["avg"][0][0].as_f64().expect("avg should be number");
    // avg of [1, 0, 1] = 0.666...
    assert!((val - 2.0 / 3.0).abs() < 0.01, "avg of booleans should be ~0.667, got {val}");

    delete(&client, &id).await;
}

#[tokio::test]
async fn sum_on_boolean_property() {
    let client = Client::new();
    let id = uid("bool-sum");
    create(&client, &boolean_entity(&id, "isActive", BOOL_OBS)).await;

    let body = get_aggregated(&client, &id, "sum", None).await;
    let val = body["isActive"]["sum"][0][0].as_f64().expect("sum should be number");
    // sum of [true=1, false=0, true=1] = 2
    pretty_assertions::assert_eq!(val, 2.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn min_on_boolean_property() {
    let client = Client::new();
    let id = uid("bool-min");
    create(&client, &boolean_entity(&id, "isActive", BOOL_OBS)).await;

    let body = get_aggregated(&client, &id, "min", None).await;
    let val = body["isActive"]["min"][0][0].as_f64().expect("min should be number");
    pretty_assertions::assert_eq!(val, 0.0, "min of [true, false, true] should be 0 (false)");

    delete(&client, &id).await;
}

#[tokio::test]
async fn max_on_boolean_property() {
    let client = Client::new();
    let id = uid("bool-max");
    create(&client, &boolean_entity(&id, "isActive", BOOL_OBS)).await;

    let body = get_aggregated(&client, &id, "max", None).await;
    let val = body["isActive"]["max"][0][0].as_f64().expect("max should be number");
    pretty_assertions::assert_eq!(val, 1.0, "max of [true, false, true] should be 1 (true)");

    delete(&client, &id).await;
}

#[tokio::test]
async fn total_count_on_boolean_property() {
    let client = Client::new();
    let id = uid("bool-tc");
    create(&client, &boolean_entity(&id, "isActive", BOOL_OBS)).await;

    let body = get_aggregated(&client, &id, "totalCount", None).await;
    let val = body["isActive"]["totalCount"][0][0].as_i64().expect("totalCount should be int");
    pretty_assertions::assert_eq!(val, 3);

    delete(&client, &id).await;
}

#[tokio::test]
async fn distinct_count_on_boolean_property() {
    let client = Client::new();
    let id = uid("bool-dc");
    // [true, false, true] → 2 distinct
    create(&client, &boolean_entity(&id, "isActive", BOOL_OBS)).await;

    let body = get_aggregated(&client, &id, "distinctCount", None).await;
    let val = body["isActive"]["distinctCount"][0][0].as_i64().expect("distinctCount should be int");
    pretty_assertions::assert_eq!(val, 2);

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// String Property aggregation (spec Table 4.5.19.1-1, JSON String)
// ---------------------------------------------------------------------------

const STRING_OBS: &[(&str, &str)] = &[
    ("banana", "2024-01-01T00:00:00Z"),
    ("apple", "2024-01-01T01:00:00Z"),
    ("cherry", "2024-01-01T02:00:00Z"),
];

#[tokio::test]
async fn min_on_string_property_is_lexicographic_first() {
    let client = Client::new();
    let id = uid("str-min");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "min", None).await;
    let val = body["status"]["min"][0][0].as_str().expect("min on string should be string");
    pretty_assertions::assert_eq!(val, "apple", "lexicographic first of [banana, apple, cherry]");

    delete(&client, &id).await;
}

#[tokio::test]
async fn max_on_string_property_is_lexicographic_last() {
    let client = Client::new();
    let id = uid("str-max");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "max", None).await;
    let val = body["status"]["max"][0][0].as_str().expect("max on string should be string");
    pretty_assertions::assert_eq!(val, "cherry", "lexicographic last of [banana, apple, cherry]");

    delete(&client, &id).await;
}

#[tokio::test]
async fn total_count_on_string_property() {
    let client = Client::new();
    let id = uid("str-tc");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "totalCount", None).await;
    let val = body["status"]["totalCount"][0][0].as_i64().expect("totalCount should be int");
    pretty_assertions::assert_eq!(val, 3);

    delete(&client, &id).await;
}

#[tokio::test]
async fn distinct_count_on_string_property() {
    let client = Client::new();
    let id = uid("str-dc");
    let obs: &[(&str, &str)] = &[
        ("alpha", "2024-01-01T00:00:00Z"),
        ("beta", "2024-01-01T01:00:00Z"),
        ("alpha", "2024-01-01T02:00:00Z"),
    ];
    create(&client, &string_entity(&id, "status", obs)).await;

    let body = get_aggregated(&client, &id, "distinctCount", None).await;
    let val = body["status"]["distinctCount"][0][0].as_i64().expect("distinctCount should be int");
    pretty_assertions::assert_eq!(val, 2);

    delete(&client, &id).await;
}

#[tokio::test]
async fn avg_on_string_property_returns_null() {
    let client = Client::new();
    let id = uid("str-avg-null");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    let val = &body["status"]["avg"][0][0];
    assert!(val.is_null(), "avg on string should be null (N/A), got {val}");

    delete(&client, &id).await;
}

#[tokio::test]
async fn sum_on_string_property_returns_null() {
    let client = Client::new();
    let id = uid("str-sum-null");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "sum", None).await;
    let val = &body["status"]["sum"][0][0];
    assert!(val.is_null(), "sum on string should be null (N/A), got {val}");

    delete(&client, &id).await;
}

#[tokio::test]
async fn stddev_on_string_property_returns_null() {
    let client = Client::new();
    let id = uid("str-stddev-null");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "stddev", None).await;
    let val = &body["status"]["stddev"][0][0];
    assert!(val.is_null(), "stddev on string should be null (N/A), got {val}");

    delete(&client, &id).await;
}

#[tokio::test]
async fn sumsq_on_string_property_returns_null() {
    let client = Client::new();
    let id = uid("str-sumsq-null");
    create(&client, &string_entity(&id, "status", STRING_OBS)).await;

    let body = get_aggregated(&client, &id, "sumsq", None).await;
    let val = &body["status"]["sumsq"][0][0];
    assert!(val.is_null(), "sumsq on string should be null (N/A), got {val}");

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Relationship aggregation (spec Table 4.5.19.1-3)
// ---------------------------------------------------------------------------

const REL_OBS: &[(&str, &str)] = &[
    ("urn:ngsi-ld:Parking:P1", "2024-01-01T00:00:00Z"),
    ("urn:ngsi-ld:Parking:P2", "2024-01-01T01:00:00Z"),
    ("urn:ngsi-ld:Parking:P1", "2024-01-01T02:00:00Z"),
];

#[tokio::test]
async fn distinct_count_on_relationship() {
    let client = Client::new();
    let id = uid("rel-dc");
    create(&client, &relationship_entity(&id, "isParked", REL_OBS)).await;

    let body = get_aggregated(&client, &id, "distinctCount", None).await;
    let val = body["isParked"]["distinctCount"][0][0].as_i64().expect("distinctCount should be int");
    // P1, P2 → 2 distinct targets
    pretty_assertions::assert_eq!(val, 2);

    delete(&client, &id).await;
}

#[tokio::test]
async fn total_count_on_relationship() {
    let client = Client::new();
    let id = uid("rel-tc");
    create(&client, &relationship_entity(&id, "isParked", REL_OBS)).await;

    let body = get_aggregated(&client, &id, "totalCount", None).await;
    let val = body["isParked"]["totalCount"][0][0].as_i64().expect("totalCount should be int");
    pretty_assertions::assert_eq!(val, 3);

    delete(&client, &id).await;
}

#[tokio::test]
async fn avg_on_relationship_returns_null() {
    let client = Client::new();
    let id = uid("rel-avg-null");
    create(&client, &relationship_entity(&id, "isParked", REL_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    let val = &body["isParked"]["avg"][0][0];
    assert!(val.is_null(), "avg on relationship should be null, got {val}");

    delete(&client, &id).await;
}

#[tokio::test]
async fn relationship_type_preserved_in_aggregation() {
    let client = Client::new();
    let id = uid("rel-type");
    create(&client, &relationship_entity(&id, "isParked", REL_OBS)).await;

    let body = get_aggregated(&client, &id, "totalCount", None).await;
    let attr_type = body["isParked"]["type"].as_str().expect("type should be string");
    pretty_assertions::assert_eq!(attr_type, "Relationship", "aggregation should preserve attribute type");

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Period bucketing (spec 4.5.19.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn aggregation_with_period_creates_buckets() {
    let client = Client::new();
    let id = uid("bucket");
    // 3 hours of data, 2 values per hour
    let obs = &[
        (10.0, "2024-01-01T00:10:00Z"),
        (20.0, "2024-01-01T00:30:00Z"),
        (30.0, "2024-01-01T01:10:00Z"),
        (40.0, "2024-01-01T01:30:00Z"),
        (50.0, "2024-01-01T02:10:00Z"),
        (60.0, "2024-01-01T02:30:00Z"),
    ];
    create(&client, &numeric_entity(&id, "speed", obs)).await;

    let body = get_aggregated(&client, &id, "avg", Some("PT1H")).await;
    let buckets = body["speed"]["avg"].as_array().expect("avg should be array of buckets");
    pretty_assertions::assert_eq!(buckets.len(), 3, "PT1H over 3 hours should create 3 buckets");

    // Bucket 0: avg(10, 20) = 15
    let b0_val = buckets[0][0].as_f64().expect("bucket value should be number");
    pretty_assertions::assert_eq!(b0_val, 15.0);

    // Bucket 1: avg(30, 40) = 35
    let b1_val = buckets[1][0].as_f64().expect("bucket value should be number");
    pretty_assertions::assert_eq!(b1_val, 35.0);

    // Bucket 2: avg(50, 60) = 55
    let b2_val = buckets[2][0].as_f64().expect("bucket value should be number");
    pretty_assertions::assert_eq!(b2_val, 55.0);

    // Each bucket should have start and end timestamps
    assert!(buckets[0][1].is_string(), "bucket_start should be datetime string");
    assert!(buckets[0][2].is_string(), "bucket_end should be datetime string");

    delete(&client, &id).await;
}

#[tokio::test]
async fn aggregation_without_period_gives_single_result() {
    let client = Client::new();
    let id = uid("no-period");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    let buckets = body["speed"]["avg"].as_array().expect("avg should be array");
    pretty_assertions::assert_eq!(buckets.len(), 1, "no period means single aggregate");
    pretty_assertions::assert_eq!(buckets[0][0].as_f64().expect("avg value"), 200.0);
    // Without period, bucket timestamps should be null
    assert!(buckets[0][1].is_null(), "bucket_start should be null when no period");
    assert!(buckets[0][2].is_null(), "bucket_end should be null when no period");

    delete(&client, &id).await;
}

#[tokio::test]
async fn zero_duration_aggregates_whole_range() {
    let client = Client::new();
    let id = uid("pt0s");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    // PT0S means aggregate whole range (spec 4.5.19.1)
    let body = get_aggregated(&client, &id, "avg", Some("PT0S")).await;
    let buckets = body["speed"]["avg"].as_array().expect("avg should be array");
    pretty_assertions::assert_eq!(buckets.len(), 1, "PT0S should aggregate whole range as single result");
    pretty_assertions::assert_eq!(buckets[0][0].as_f64().expect("avg value"), 200.0);

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Multiple methods and attributes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multiple_aggr_methods_in_single_query() {
    let client = Client::new();
    let id = uid("multi-method");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "avg,min,max,totalCount", None).await;
    let speed = &body["speed"];
    pretty_assertions::assert_eq!(speed["avg"][0][0].as_f64().expect("avg"), 200.0);
    pretty_assertions::assert_eq!(speed["min"][0][0].as_f64().expect("min"), 100.0);
    pretty_assertions::assert_eq!(speed["max"][0][0].as_f64().expect("max"), 300.0);
    pretty_assertions::assert_eq!(speed["totalCount"][0][0].as_i64().expect("totalCount"), 3);

    delete(&client, &id).await;
}

#[tokio::test]
async fn aggregation_across_multiple_attributes() {
    let client = Client::new();
    let id = uid("multi-attr");
    let entity = json!({
        "id": id,
        "type": "AggTestEntity",
        "speed": [
            {"type": "Property", "value": 100, "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": 200, "observedAt": "2024-01-01T01:00:00Z"}
        ],
        "fuel": [
            {"type": "Property", "value": 50, "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": 30, "observedAt": "2024-01-01T01:00:00Z"}
        ],
        "@context": CTX
    });
    create(&client, &entity).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    pretty_assertions::assert_eq!(body["speed"]["avg"][0][0].as_f64().expect("speed avg"), 150.0);
    pretty_assertions::assert_eq!(body["fuel"]["avg"][0][0].as_f64().expect("fuel avg"), 40.0);

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Response structure (spec 4.5.19)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn aggregated_response_has_correct_structure() {
    let client = Client::new();
    let id = uid("structure");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;

    // Entity-level fields
    pretty_assertions::assert_eq!(body["id"].as_str().expect("id"), id);
    pretty_assertions::assert_eq!(body["type"].as_str().expect("type"), "AggTestEntity");

    // Attribute structure: object with type + aggregation keys
    let speed = &body["speed"];
    pretty_assertions::assert_eq!(speed["type"].as_str().expect("attr type"), "Property");
    assert!(speed["avg"].is_array(), "aggregation key should contain array of triples");

    // Triple structure: [value, bucket_start, bucket_end]
    let triple = &speed["avg"][0];
    assert!(triple.is_array(), "each entry should be a triple array");
    pretty_assertions::assert_eq!(triple.as_array().expect("array").len(), 3, "triple should have exactly 3 elements");

    delete(&client, &id).await;
}

#[tokio::test]
async fn aggregated_response_property_type_is_property() {
    let client = Client::new();
    let id = uid("prop-type");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    pretty_assertions::assert_eq!(body["speed"]["type"].as_str().expect("type"), "Property");

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Mixed types in same entity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mixed_property_types_aggregated_correctly() {
    let client = Client::new();
    let id = uid("mixed");
    let entity = json!({
        "id": id,
        "type": "AggTestEntity",
        "speed": [
            {"type": "Property", "value": 100, "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": 200, "observedAt": "2024-01-01T01:00:00Z"}
        ],
        "status": [
            {"type": "Property", "value": "open", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": "closed", "observedAt": "2024-01-01T01:00:00Z"}
        ],
        "isParked": [
            {"type": "Relationship", "object": "urn:ngsi-ld:Parking:P1", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Relationship", "object": "urn:ngsi-ld:Parking:P2", "observedAt": "2024-01-01T01:00:00Z"}
        ],
        "@context": CTX
    });
    create(&client, &entity).await;

    let body = get_aggregated(&client, &id, "avg,min,max,totalCount,distinctCount", None).await;

    // Numeric: all methods work
    pretty_assertions::assert_eq!(body["speed"]["avg"][0][0].as_f64().expect("speed avg"), 150.0);
    pretty_assertions::assert_eq!(body["speed"]["min"][0][0].as_f64().expect("speed min"), 100.0);
    pretty_assertions::assert_eq!(body["speed"]["max"][0][0].as_f64().expect("speed max"), 200.0);
    pretty_assertions::assert_eq!(body["speed"]["totalCount"][0][0].as_i64().expect("speed tc"), 2);
    pretty_assertions::assert_eq!(body["speed"]["distinctCount"][0][0].as_i64().expect("speed dc"), 2);

    // String: min/max are lexicographic, avg is null
    assert!(body["status"]["avg"][0][0].is_null(), "avg on string should be null");
    pretty_assertions::assert_eq!(body["status"]["min"][0][0].as_str().expect("status min"), "closed");
    pretty_assertions::assert_eq!(body["status"]["max"][0][0].as_str().expect("status max"), "open");
    pretty_assertions::assert_eq!(body["status"]["totalCount"][0][0].as_i64().expect("status tc"), 2);
    pretty_assertions::assert_eq!(body["status"]["distinctCount"][0][0].as_i64().expect("status dc"), 2);

    // Relationship: avg is null, distinctCount counts targets
    assert!(body["isParked"]["avg"][0][0].is_null(), "avg on relationship should be null");
    pretty_assertions::assert_eq!(body["isParked"]["totalCount"][0][0].as_i64().expect("rel tc"), 2);
    pretty_assertions::assert_eq!(body["isParked"]["distinctCount"][0][0].as_i64().expect("rel dc"), 2);
    pretty_assertions::assert_eq!(body["isParked"]["type"].as_str().expect("type"), "Relationship");

    delete(&client, &id).await;
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn aggregation_single_value() {
    let client = Client::new();
    let id = uid("single");
    let obs = &[(42.0, "2024-01-01T00:00:00Z")];
    create(&client, &numeric_entity(&id, "speed", obs)).await;

    let body = get_aggregated(&client, &id, "avg,min,max,sum,totalCount,distinctCount", None).await;
    pretty_assertions::assert_eq!(body["speed"]["avg"][0][0].as_f64().expect("avg"), 42.0);
    pretty_assertions::assert_eq!(body["speed"]["min"][0][0].as_f64().expect("min"), 42.0);
    pretty_assertions::assert_eq!(body["speed"]["max"][0][0].as_f64().expect("max"), 42.0);
    pretty_assertions::assert_eq!(body["speed"]["sum"][0][0].as_f64().expect("sum"), 42.0);
    pretty_assertions::assert_eq!(body["speed"]["totalCount"][0][0].as_i64().expect("totalCount"), 1);
    pretty_assertions::assert_eq!(body["speed"]["distinctCount"][0][0].as_i64().expect("distinctCount"), 1);

    delete(&client, &id).await;
}

#[tokio::test]
async fn aggregation_with_dataset_id() {
    let client = Client::new();
    let id = uid("dataset");
    let entity = json!({
        "id": id,
        "type": "AggTestEntity",
        "speed": [
            {"type": "Property", "value": 100, "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": 200, "observedAt": "2024-01-01T01:00:00Z"},
            {"type": "Property", "value": 300, "observedAt": "2024-01-01T00:00:00Z", "datasetId": "urn:ngsi-ld:speed:radar"},
            {"type": "Property", "value": 400, "observedAt": "2024-01-01T01:00:00Z", "datasetId": "urn:ngsi-ld:speed:radar"}
        ],
        "@context": CTX
    });
    create(&client, &entity).await;

    let body = get_aggregated(&client, &id, "avg", None).await;
    // Two datasets should produce separate aggregation instances
    let speed = &body["speed"];
    assert!(speed.is_array(), "multi-dataset should produce an array of instances");
    let arr = speed.as_array().expect("array");
    pretty_assertions::assert_eq!(arr.len(), 2, "two datasets should produce two instances");
    // Find the default (no datasetId) and the named one
    let default_inst = arr.iter().find(|v| v.get("datasetId").is_none());
    let radar_inst = arr.iter().find(|v| v.get("datasetId").is_some());
    assert!(default_inst.is_some(), "should have default dataset instance");
    assert!(radar_inst.is_some(), "should have radar dataset instance");
    pretty_assertions::assert_eq!(default_inst.expect("default")["avg"][0][0].as_f64().expect("default avg"), 150.0);
    pretty_assertions::assert_eq!(radar_inst.expect("radar")["avg"][0][0].as_f64().expect("radar avg"), 350.0);

    delete(&client, &id).await;
}

#[tokio::test]
async fn all_eight_methods_on_numeric() {
    let client = Client::new();
    let id = uid("all-methods");
    let obs = &[(10.0, "2024-01-01T00:00:00Z"), (20.0, "2024-01-01T01:00:00Z"), (30.0, "2024-01-01T02:00:00Z")];
    create(&client, &numeric_entity(&id, "speed", obs)).await;

    let body = get_aggregated(&client, &id, "avg,min,max,sum,sumsq,stddev,totalCount,distinctCount", None).await;
    let s = &body["speed"];
    pretty_assertions::assert_eq!(s["avg"][0][0].as_f64().expect("avg"), 20.0);
    pretty_assertions::assert_eq!(s["min"][0][0].as_f64().expect("min"), 10.0);
    pretty_assertions::assert_eq!(s["max"][0][0].as_f64().expect("max"), 30.0);
    pretty_assertions::assert_eq!(s["sum"][0][0].as_f64().expect("sum"), 60.0);
    // sumsq = 100 + 400 + 900 = 1400
    pretty_assertions::assert_eq!(s["sumsq"][0][0].as_f64().expect("sumsq"), 1400.0);
    // stddev of [10,20,30] = 10.0 (sample)
    let stddev = s["stddev"][0][0].as_f64().expect("stddev");
    assert!((stddev - 10.0).abs() < 0.01);
    pretty_assertions::assert_eq!(s["totalCount"][0][0].as_i64().expect("totalCount"), 3);
    pretty_assertions::assert_eq!(s["distinctCount"][0][0].as_i64().expect("distinctCount"), 3);

    delete(&client, &id).await;
}

#[tokio::test]
async fn aggregation_bucket_timestamps_are_consistent() {
    let client = Client::new();
    let id = uid("bucket-ts");
    let obs = &[(10.0, "2024-01-01T00:10:00Z"), (20.0, "2024-01-01T01:10:00Z")];
    create(&client, &numeric_entity(&id, "speed", obs)).await;

    let body = get_aggregated(&client, &id, "avg", Some("PT1H")).await;
    let buckets = body["speed"]["avg"].as_array().expect("array");

    for bucket in buckets {
        let start = bucket[1].as_str().expect("bucket_start should be string");
        let end = bucket[2].as_str().expect("bucket_end should be string");
        // end should be start + PT1H
        let start_dt = chrono::DateTime::parse_from_rfc3339(start).expect("parse start");
        let end_dt = chrono::DateTime::parse_from_rfc3339(end).expect("parse end");
        let diff = end_dt - start_dt;
        pretty_assertions::assert_eq!(diff, chrono::Duration::hours(1), "bucket width should be exactly PT1H");
    }

    delete(&client, &id).await;
}

#[tokio::test]
async fn p0d_duration_aggregates_whole_range() {
    let client = Client::new();
    let id = uid("p0d");
    create(&client, &numeric_entity(&id, "speed", NUMERIC_OBS)).await;

    // P0D (zero days) should behave like PT0S -- aggregate whole range
    let body = get_aggregated(&client, &id, "avg", Some("P0D")).await;
    let buckets = body["speed"]["avg"].as_array().expect("array");
    pretty_assertions::assert_eq!(buckets.len(), 1, "P0D should aggregate whole range");
    pretty_assertions::assert_eq!(buckets[0][0].as_f64().expect("avg"), 200.0);

    delete(&client, &id).await;
}
