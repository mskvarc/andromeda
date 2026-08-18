//! Verification-matrix row "Current projection replayed writes".
//!
//! Under `Prefer: respond-async` the batch request lands the Entity rows and
//! enqueues its observations as a durable side-effect job; a worker later
//! replays that job as writes carrying no current-state mutation. Those
//! replayed observations are still the projection's input, so the projection
//! must land once the worker replays them, keyed on the entity rows the
//! original request wrote.

use crate::projection_rows::{ProjectionRow, rows_for_attribute};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// How long the side-effect worker is given to replay the job.
const REPLAY_TIMEOUT: Duration = Duration::from_secs(30);

/// Interval between projection reads while waiting for the replay.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[tokio::test]
async fn current_projection_lands_for_observations_replayed_by_the_side_effect_worker() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:replayed-projection");

    let batch = json!([{
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 17.25},
        "@context": [TEST_SUITE_CONTEXT]
    }]);
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/upsert")
        .header("Content-Type", "application/ld+json")
        .header("Prefer", "respond-async")
        .json(&batch)
        .send()
        .await
        .expect("async batch upsert failed");
    assert_eq!(response.status, StatusCode::CREATED, "upsert: {:?}", response.body);
    assert_eq!(
        response.headers.get("preference-applied").and_then(|value| value.to_str().ok()),
        Some("respond-async"),
        "the broker must have taken the asynchronous side-effect path"
    );

    let expected = vec![ProjectionRow {
        attribute_name: "https://uri.etsi.org/ngsi-ld/default-context/temperature".to_owned(),
        dataset_id: None,
        attribute_kind: 1,
        value_kind: 1,
        value_text: Some("17.25".to_owned()),
    }];
    let deadline = Instant::now() + REPLAY_TIMEOUT;
    let mut rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    while rows != expected && Instant::now() < deadline {
        sleep(POLL_INTERVAL).await;
        rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    }
    assert_eq!(rows, expected, "the replayed observations must reach the projection");

    scope.finish().await.expect("cleanup failed");
}
