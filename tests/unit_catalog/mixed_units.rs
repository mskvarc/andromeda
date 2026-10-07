//! Mixed-unit aggregation: no aggregate silently spans two units.
//!
//! Clause 5.2.5 attaches `unitCode` to the instance, so two instances of one
//! attribute may differ in unit, and clause 4.5.19 says nothing about what an
//! average across two units would mean. Andromeda never converts units, so a
//! unit-sensitive aggregate spanning two units
//! is refused rather than labelled with one of them, while an aggregate that
//! only counts stays answerable because its result carries no unit at all.
//!
//! Clause 4.5.19 also gives exactly one aggregate value per attribute,
//! `datasetId` and period, so the answerable case has to arrive as one triple
//! covering both units, not as one triple per unit repeating the same period
//! bounds.

use crate::fixtures::SUITE;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A temporal entity whose one attribute was recorded under two unit codes.
fn scale_with_two_units(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Scale",
        "weight": [
            {"type": "Property", "value": 5, "unitCode": "KGM", "observedAt": "2026-01-01T00:00:00Z"},
            {"type": "Property", "value": 7, "unitCode": "GRM", "observedAt": "2026-01-01T01:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    })
}

/// A temporal entity whose one attribute was recorded under a single unit code.
fn scale_with_one_unit(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Scale",
        "weight": [
            {"type": "Property", "value": 5, "unitCode": "KGM", "observedAt": "2026-01-01T00:00:00Z"},
            {"type": "Property", "value": 7, "unitCode": "KGM", "observedAt": "2026-01-01T01:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    })
}

/// Request one aggregation of an entity's history.
async fn aggregate(scope: &TestEnvironment, id: &str, methods: &str) -> (StatusCode, String) {
    let response = scope
        .client()
        .get_temporal_entity(id, &format!("format=aggregatedValues&aggrMethods={methods}"), None)
        .await
        .expect("aggregation request failed");
    (response.status, String::from_utf8_lossy(&response.body).into_owned())
}

/// The `weight` member of an aggregated response, which is one instance object.
fn aggregated_weight(body: &str) -> Value {
    let parsed: Value = serde_json::from_str(body)
        .map_err(|error| format!("{error} in {body}"))
        .expect("an aggregated response is JSON");
    let weight = parsed
        .get("weight")
        .ok_or_else(|| format!("no aggregated weight in {body}"))
        .expect("the aggregated attribute is present");
    assert!(weight.is_object(), "one attribute instance renders as one object: {body}");
    assert_eq!(weight["type"], json!("Property"), "the aggregate is reported under its own family: {body}");
    weight.clone()
}

#[tokio::test]
async fn a_unit_sensitive_aggregate_spanning_two_units_is_refused_rather_than_silently_labelled() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let id = scope.id("Scale:mixed-units");

    let created = scope
        .client()
        .create_temporal_entity(&scale_with_two_units(&id))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let (status, body) = aggregate(&scope, &id, "avg").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an average across two units is not a quantity the broker can name: {body}"
    );

    // Clause 5.5.9: the units the selected instances carry are a property of the
    // request, so the refusal is bad input rather than a fault to retry, and it
    // names the attribute it refused so the client can see what to narrow.
    let problem: Value = serde_json::from_str(&body).unwrap_or_else(|error| panic!("a refusal carries ProblemDetails: {error} in {body}"));
    assert_eq!(problem["type"], json!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"), "{body}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("weight"), "the refusal names the attribute it refused: {body}");
    assert!(detail.contains("unit codes"), "the refusal says what about the attribute it refused: {body}");
    assert!(problem.get("weight").is_none(), "a refused aggregate returns no aggregate at all: {body}");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_counting_aggregate_stays_answerable_across_two_units() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let id = scope.id("Scale:mixed-units-count");

    let created = scope
        .client()
        .create_temporal_entity(&scale_with_two_units(&id))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // The result of a count carries no unit, so clause 4.5.19 leaves nothing
    // undefined and the request is answered over both units together.
    let (status, body) = aggregate(&scope, &id, "totalCount").await;
    assert_eq!(status, StatusCode::OK, "a count over mixed units is well defined: {body}");

    // Clause 4.5.19: one aggregate value per attribute, dataset and period. Both
    // observations fall in the one period an absent `aggrPeriodDuration` makes,
    // so the answer is a single triple counting both — never one triple per unit
    // with the same period bounds on each.
    let weight = aggregated_weight(&body);
    assert_eq!(
        weight["totalCount"],
        json!([[2, Value::Null, Value::Null]]),
        "both units are counted once over one period: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_unit_sensitive_aggregate_over_one_unit_is_answered_normally() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let id = scope.id("Scale:single-unit");

    let created = scope
        .client()
        .create_temporal_entity(&scale_with_one_unit(&id))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let (status, body) = aggregate(&scope, &id, "avg,min,max,totalCount").await;
    assert_eq!(status, StatusCode::OK, "one unit leaves the aggregate well defined: {body}");

    // The whole range is one period, so every method answers with exactly one
    // triple, and the extrema are the observed values rather than the numbers
    // some other unit's instances contributed.
    let weight = aggregated_weight(&body);
    let avg = weight["avg"].as_array().unwrap_or_else(|| panic!("avg is a list of triples: {body}"));
    assert_eq!(avg.len(), 1, "the whole range is one period: {body}");
    // The mean is exact but its scale is the database's, so it is compared as a
    // number rather than against one particular spelling of six.
    assert_eq!(avg[0][0].as_f64(), Some(6.0), "avg of 5 and 7: {body}");
    assert_eq!(avg[0][1], Value::Null, "an absent period has no bucket start: {body}");
    assert_eq!(avg[0][2], Value::Null, "an absent period has no bucket end: {body}");
    assert_eq!(weight["min"], json!([[5, Value::Null, Value::Null]]), "min of 5 and 7: {body}");
    assert_eq!(weight["max"], json!([[7, Value::Null, Value::Null]]), "max of 5 and 7: {body}");
    assert_eq!(weight["totalCount"], json!([[2, Value::Null, Value::Null]]), "two observations: {body}");

    scope.finish().await.expect("cleanup failed");
}
