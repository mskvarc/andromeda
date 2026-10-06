//! A Subscription reads back as it was written, through its typed model.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.8.1 (Create Subscription) stores the members
//! the client supplies and clause 5.8.2 (Retrieve Subscription) serves them
//! back; Table 5.2.12-1 lists them. The resource is held as a typed model, so a
//! POST followed by a GET has to answer the same members with the same values,
//! apart from the ones the broker generates itself (`createdAt`, `modifiedAt`,
//! `status`, `isActive`, and the delivery counters of Table 5.2.14.2-1).
//!
//! Needs the host broker: the cases drive the HTTP API and are not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::round_trip_comparison::assert_round_trips;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::json;

/// Clauses 5.8.1 and 5.8.2: every member a client supplies on a Subscription is
/// served back unchanged.
#[tokio::test]
async fn regression_subscription_round_trips_through_its_typed_model() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:typed-round-trip");
    let posted = json!({
        "id": id,
        "type": "Subscription",
        "subscriptionName": "typed round trip",
        "description": "every member the client may supply",
        "entities": [{"type": scope.entity_type("Vehicle")}],
        "watchedAttributes": ["speed"],
        "notificationTrigger": ["attributeCreated", "attributeUpdated"],
        "q": "speed>10",
        "throttling": 5,
        "expiresAt": "2099-01-01T00:00:00Z",
        "notification": {
            "attributes": ["speed"],
            "format": "normalized",
            "endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });

    let created = client.create_subscription(&posted).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let retrieved = client
        .retrieve_subscription(&id, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve subscription request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let returned = retrieved.json_value().expect("a retrieved subscription is JSON");

    assert_round_trips(posted, returned, "Subscription");

    client.delete_subscription(&id).await.expect("delete subscription request");
    scope.finish().await.expect("environment cleanup");
}
