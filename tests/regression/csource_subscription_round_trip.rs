//! A Context Source Registration Subscription reads back as it was written,
//! through its typed model.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.11.2 (Create Context Source Registration
//! Subscription) takes the Subscription data type of clause 5.2.12, and clause
//! 5.11.4.5 returns "the subscription details as mandated by clause 5.2.12". The
//! members the client supplies therefore come back with the values they were
//! written with, apart from the ones the broker generates (`createdAt`,
//! `modifiedAt`, `status`, `isActive` and the delivery counters of Table
//! 5.2.14.2-1).
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::round_trip_comparison::assert_round_trips;
use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use urlencoding::encode;

/// Clauses 5.11.2 and 5.11.4.5: a Context Source Registration Subscription is
/// served back with every member the client supplied.
#[tokio::test]
async fn regression_csource_subscription_round_trips_through_its_typed_model() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:csource-typed-round-trip");
    let posted = json!({
        "id": id,
        "type": "Subscription",
        "subscriptionName": "csource typed round trip",
        "description": "every member the client may supply",
        "entities": [{"type": "Vehicle"}],
        "q": "speed>10",
        "expiresAt": "2099-01-01T00:00:00Z",
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    });

    let created = client
        .raw(Method::POST, "csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(&posted)
        .send()
        .await
        .expect("create context source subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let retrieved = client
        .raw(Method::GET, &format!("csourceSubscriptions/{}", encode(&id)))
        .header("accept", "application/ld+json")
        .header("link", jsonld_link_header(CORE_CONTEXT).expect("core context Link must be valid"))
        .send()
        .await
        .expect("retrieve context source subscription request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let returned = retrieved.json_value().expect("a retrieved subscription is JSON");

    assert_round_trips(posted, returned, "Context Source Registration Subscription");

    client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(&id)))
        .send()
        .await
        .expect("delete context source subscription request");
    scope.finish().await.expect("environment cleanup");
}
