//! Attribute-level `expiresAt` integration tests (NGSI-LD 4.22).
//!
//! Naming convention: `expiration_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod entity_expiry;

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tokio::time::{Instant, sleep};

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn fetch(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

async fn merge(client: &TestClient, id: &str, fragment: &Value) {
    let response = client.merge_entity(id, fragment, None).await.expect("merge_entity failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

async fn query(client: &TestClient, query_str: &str) -> Value {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

// --- expiresAt preservation on create/retrieve (NGSI-LD 4.22) ---

#[tokio::test]
async fn expiration_property_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-prop");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 80, "expiresAt": "2099-12-31T23:59:59Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_relationship_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-rel");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "isParkedIn": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:ParkingSpot:1",
                "expiresAt": "2099-06-15T12:00:00Z"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["isParkedIn"]["expiresAt"], "2099-06-15T12:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_geo_property_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-geo");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.0, 52.0]},
                "expiresAt": "2099-01-01T00:00:00Z"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["location"]["expiresAt"], "2099-01-01T00:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_vocab_property_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-vocab");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "category": {"type": "VocabProperty", "vocab": "bus", "expiresAt": "2099-03-15T08:00:00Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["category"]["expiresAt"], "2099-03-15T08:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_language_property_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-lang");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "name": {
                "type": "LanguageProperty",
                "languageMap": {"en": "bus", "de": "Bus"},
                "expiresAt": "2099-07-01T00:00:00Z"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["name"]["expiresAt"], "2099-07-01T00:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_json_property_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-json");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "diagnostics": {
                "type": "JsonProperty",
                "json": {"engine": "ok", "brakes": "warning"},
                "expiresAt": "2099-09-01T12:00:00Z"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["diagnostics"]["expiresAt"], "2099-09-01T12:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_list_property_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-listp");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "tags": {
                "type": "ListProperty",
                "valueList": ["a", "b"],
                "expiresAt": "2099-11-20T06:30:00Z"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["tags"]["expiresAt"], "2099-11-20T06:30:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_list_relationship_expires_at_preserved_on_create_retrieve() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-listr");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "connectedTo": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"],
                "expiresAt": "2099-04-10T18:00:00Z"
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["connectedTo"]["expiresAt"], "2099-04-10T18:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

// --- Independence and edge cases ---

#[tokio::test]
async fn expiration_attribute_independent_of_entity_level_expires_at() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-indep");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "expiresAt": "2099-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 60, "expiresAt": "2098-06-01T00:00:00Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["expiresAt"], "2099-01-01T00:00:00Z");
    assert_eq!(body["speed"]["expiresAt"], "2098-06-01T00:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_multi_instance_different_expires_at_each_preserved() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-multi");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": [
                {
                    "type": "Property",
                    "value": 80,
                    "datasetId": "urn:ngsi-ld:dataset:speedometer",
                    "expiresAt": "2099-12-31T23:59:59Z"
                },
                {
                    "type": "Property",
                    "value": 78,
                    "datasetId": "urn:ngsi-ld:dataset:gps",
                    "expiresAt": "2098-06-15T00:00:00Z"
                }
            ],
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    let speed = body["speed"].as_array().expect("speed must be multi-instance array");
    assert_eq!(speed.len(), 2);
    let values: Vec<&str> = speed.iter().filter_map(|s| s["expiresAt"].as_str()).collect();
    assert!(values.contains(&"2099-12-31T23:59:59Z"));
    assert!(values.contains(&"2098-06-15T00:00:00Z"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_attribute_survives_unrelated_merge_patch() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-surv");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 50, "expiresAt": "2099-12-31T23:59:59Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    merge(
        client,
        &id,
        &json!({
            "fuel": {"type": "Property", "value": 75},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_expires_at_visible_without_sys_attrs_option() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-nosys");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 90, "expiresAt": "2099-12-31T23:59:59Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "").await;
    assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_expires_at_updatable_via_merge_patch() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-update");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 60, "expiresAt": "2099-01-01T00:00:00Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    merge(
        client,
        &id,
        &json!({
            "speed": {"type": "Property", "value": 60, "expiresAt": "2099-06-30T00:00:00Z"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert_eq!(body["speed"]["expiresAt"], "2099-06-30T00:00:00Z");
    scope.finish().await.expect("cleanup failed");
}

// --- Worker-driven expiration tests ---
//
// The base stack runs the expiry sweep every second (EXPIRY_SWEEP_INTERVAL), so
// these tests poll for the swept state rather than sleeping a fixed cycle. Rows
// are scoped by id prefix so the worker's actions on other tests' rows do not
// affect us.

/// The base test stack's `temporal.expiration_check_interval`
/// (`ANDROMEDA__TEMPORAL__EXPIRATION_CHECK_INTERVAL: "1s"` in the base compose
/// file). Kept in sync with it so the worker waits below track the real sweep
/// cadence.
const EXPIRY_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// Wait until the expiry sweep has left the entity in `expected` shape.
///
/// The worker runs on `expiration_check_interval` (one second in the base test
/// stack). Polling for the swept state, rather than sleeping a fixed cycle,
/// keeps the test fast and immune to landing early or late in the interval: a
/// fixed twelve-second sleep once raced the sweep and failed one test per run
/// under an alternating name. The deadline is a generous multiple of the sweep
/// interval, enough to absorb a loaded broker without the old twenty-second
/// ceiling that only ever masked a misconfigured sweep.
async fn wait_for_expiry_sweep<F, Fut>(what: &str, mut settled: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + EXPIRY_SWEEP_INTERVAL * 6;
    loop {
        if settled().await {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        sleep(Duration::from_millis(200)).await;
    }
}

/// The earliest instance expiry the write path recorded on the entity's row.
async fn recorded_minimum_expiry(scope: &TestEnvironment, id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT to_char(min_attribute_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') FROM entities WHERE id = $1")
        .bind(id)
        .fetch_one(scope.database_pool())
        .await
        .expect("the entity row must be readable")
}

#[tokio::test]
async fn expiration_worker_partial_multi_instance_expired_instance_removed() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-part");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 80, "datasetId": "urn:ngsi-ld:dataset:a", "expiresAt": "2020-01-01T00:00:00Z"},
                {"type": "Property", "value": 78, "datasetId": "urn:ngsi-ld:dataset:b", "expiresAt": "2099-12-31T23:59:59Z"},
                {"type": "Property", "value": 75, "datasetId": "urn:ngsi-ld:dataset:c"}
            ],
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    // The sweep finds the entity through the earliest instance expiry the write
    // path records on its row; after the purge that record must have moved on
    // to the surviving instance's expiry, or the next sweep would read the
    // entity as due again. That row is the only sweep-observable signal here:
    // the read path filters an expired instance out on its own, so the array
    // length below is already correct before any sweep runs.
    wait_for_expiry_sweep("the purge to re-record the surviving instance's expiry", || async {
        recorded_minimum_expiry(&scope, &id).await.as_deref() == Some("2099-12-31T23:59:59Z")
    })
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    let speed = body["speed"].as_array().expect("speed must be multi-instance array");
    assert_eq!(speed.len(), 2, "expired instance must be removed");

    let recorded = recorded_minimum_expiry(&scope, &id).await;
    assert_eq!(
        recorded.as_deref(),
        Some("2099-12-31T23:59:59Z"),
        "the recorded minimum must be the surviving instance's expiry"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_worker_all_instances_expired_attribute_absent_from_entity() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-allone");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 80, "expiresAt": "2020-01-01T00:00:00Z"},
            "fuel": {"type": "Property", "value": 55},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    wait_for_expiry_sweep("the expired attribute to disappear", || async {
        fetch(client, &id, "options=sysAttrs").await.get("speed").is_none()
    })
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    assert!(body.get("speed").is_none());
    assert!(body.get("fuel").is_some());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_worker_attribute_without_expires_at_never_removed() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-noexp");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 60},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    // Nothing to poll for: the claim is that a sweep leaves the attribute
    // alone, so the wait must outlast at least one whole sweep interval.
    sleep(EXPIRY_SWEEP_INTERVAL + Duration::from_secs(2)).await;
    let body = fetch(client, &id, "").await;
    assert!(body.get("speed").is_some());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_worker_expired_location_removed_and_excluded_from_geo_queries() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-geo-null");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.0, 52.0]},
                "expiresAt": "2020-01-01T00:00:00Z"
            },
            "speed": {"type": "Property", "value": 60},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    wait_for_expiry_sweep("the expired location to be purged", || async {
        fetch(client, &id, "options=sysAttrs").await.get("location").is_none()
    })
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    assert!(body.get("location").is_none(), "expired location must be removed");
    assert!(body.get("speed").is_some());

    let body = query(
        client,
        &format!(
            "type=Vehicle&id={}&georel=near;maxDistance%3D%3D1000&geometry=Point&coordinates=%5B13.0%2C52.0%5D",
            urlencoding::encode(&id)
        ),
    )
    .await;
    let entities = body.as_array().expect("geo query must return JSON array");
    let matched = entities.iter().any(|entity| entity["id"].as_str() == Some(id.as_str()));
    assert!(!matched, "entity with expired location must not appear in geo query");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn expiration_worker_attribute_expiry_does_not_affect_entity_level_expires_at() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-entattr");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "expiresAt": "2099-12-31T23:59:59Z",
            "speed": {"type": "Property", "value": 80, "expiresAt": "2020-01-01T00:00:00Z"},
            "fuel": {"type": "Property", "value": 50},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    wait_for_expiry_sweep("the expired attribute to disappear", || async {
        fetch(client, &id, "options=sysAttrs").await.get("speed").is_none()
    })
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    assert!(body.get("speed").is_none());
    assert!(body.get("fuel").is_some());
    assert_eq!(body["expiresAt"], "2099-12-31T23:59:59Z");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.22 lets final deletion "lag the expiresAt timestamp to a certain
/// extent" -- the clean-up cadence, not an unbounded wait. A batch write once
/// suspended the entity expiry sweep for ten seconds after every batch
/// operation in the tenant, so an attribute that expired alongside batch
/// traffic outlived the sweep's bound. The entity here is itself written by a
/// batch, immediately before the sweep is awaited, and its expired attribute
/// must still be purged within the ordinary sweep bound. The purge is observed
/// through the entity row's recorded minimum expiry, which only the sweep
/// changes (the read path filters an expired instance on its own).
#[tokio::test]
async fn expiration_worker_expired_attribute_purged_within_sweep_bound_right_after_a_batch_write() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-after-batch");
    let created = client
        .batch_create(&json!([{
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 80, "expiresAt": "2020-01-01T00:00:00Z"},
            "fuel": {"type": "Property", "value": 55},
            "@context": [TEST_SUITE_CONTEXT]
        }]))
        .await
        .expect("batch create request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_eq!(
        recorded_minimum_expiry(&scope, &id).await.as_deref(),
        Some("2020-01-01T00:00:00Z"),
        "the batch write records the attribute's expiry for the sweep to find"
    );

    wait_for_expiry_sweep("the sweep to purge the expired attribute after a batch write", || async {
        recorded_minimum_expiry(&scope, &id).await.is_none()
    })
    .await;

    let body = fetch(client, &id, "options=sysAttrs").await;
    assert!(body.get("speed").is_none(), "{body}");
    assert!(body.get("fuel").is_some(), "{body}");
    scope.finish().await.expect("cleanup failed");
}
