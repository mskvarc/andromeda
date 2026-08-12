//! Verification matrix row "Mixed-unit aggregation".
//!
//! Clause 5.2.5 attaches `unitCode` to the instance, so two instances of one
//! attribute may differ in unit, and clause 4.5.19 says nothing about what an
//! average across two units would mean. Andromeda never converts
//! (`docs/unit-handling.md`), so a unit-sensitive aggregate spanning two units
//! is refused rather than labelled with one of them, while an aggregate that
//! only counts stays answerable because its result carries no unit at all.

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

    let (status, body) = aggregate(&scope, &id, "avg").await;
    assert_eq!(status, StatusCode::OK, "one unit leaves the aggregate well defined: {body}");

    scope.finish().await.expect("cleanup failed");
}
