//! A Subscription's `geoQ` and `temporalQ` are stored through typed members and
//! served back as written, and `geoQ: null` removes the member.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.8.1 stores the `geoQ` of Table 5.2.13-1
//! (`georel`, `geometry`, `coordinates`) and the `temporalQ` of Table 5.2.21-1
//! (`timerel`, `timeAt`) a client supplies, and clause 5.8.2 serves them back.
//! Clause 5.8.3 (Update Subscription) applies a JSON Merge Patch, so a member
//! set to `null` in the fragment is deleted from the stored Subscription
//! (clause 5.5.8).
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// Clauses 5.8.1, 5.8.2 and 5.8.3: both query members read back equal to what
/// was posted, and a merge patch with `"geoQ": null` removes `geoQ` while
/// `temporalQ` stays.
#[tokio::test]
async fn regression_subscription_geo_q_is_stored_typed_and_removed_by_null() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:geo-q-typed");
    let geo_q = json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [14.5, 46.05]});
    let temporal_q = json!({"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"});
    let body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": scope.entity_type("GeoProbe")}],
        "geoQ": geo_q,
        "temporalQ": temporal_q,
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });

    let created = client.create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let stored = client
        .retrieve_subscription(&id, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve subscription request")
        .json_value()
        .expect("a retrieved subscription is JSON");
    assert_eq!(stored["geoQ"], geo_q, "geoQ reads back as posted: {stored}");
    assert_eq!(stored["temporalQ"], temporal_q, "temporalQ reads back as posted: {stored}");

    let patched = client
        .update_subscription(&id, &json!({"geoQ": null}), Some(TEST_SUITE_CONTEXT))
        .await
        .expect("update subscription request");
    assert_eq!(patched.status, StatusCode::NO_CONTENT, "update: {:?}", patched.body);
    let after = client
        .retrieve_subscription(&id, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve subscription request")
        .json_value()
        .expect("a retrieved subscription is JSON");
    assert!(after.get("geoQ").is_none(), "a null geoQ removes the member: {after}");
    assert_eq!(after["temporalQ"], temporal_q, "the members the patch did not name are kept: {after}");

    client.delete_subscription(&id).await.expect("delete subscription request");
    scope.finish().await.expect("environment cleanup");
}
