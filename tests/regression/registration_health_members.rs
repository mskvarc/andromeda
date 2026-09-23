//! What a Context Source Registration reports about itself, and how its
//! `endpoint` is spelled on the way back out.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.9-2 adds five read-only members to the
//! `CSourceRegistration` of Table 5.2.9-1, and gives every one of them
//! cardinality 0..1. They report observations: `status` is "ok" "if the last
//! attempt to perform a distributed operation succeeded" and "failed" if it
//! failed, `timesSent` counts the operations the registration triggered,
//! `timesFailed` those that failed, and `lastSuccess` is "created on first
//! successful operation". A registration that has triggered nothing has
//! observed nothing, so it reports none of them — the broker answered with a
//! `status` and two zeroes instead, which asserts a health nothing had been
//! observed.
//!
//! Table 5.2.14.2-1 gives a Subscription's `notification` the same three
//! members on the same terms, and the subscription side already suppressed
//! them; the registration side is brought level with it here rather than given
//! a second rule.
//!
//! The `endpoint` half is Table 5.2.9-1, which restricts the member to "a
//! dereferenceable URI" but says nothing about rewriting it. Clause 5.2.2 makes
//! the members a Context Producer supplies ones the broker stores and serves
//! back, so a registration written as `http://my.csource.org:1026` has to read
//! back that way — the broker parsed it into a `url::Url`, which normalizes an
//! authority-only URI by appending the empty path it implies, and answered
//! `http://my.csource.org:1026/`. The same applied to `jsonldContext` and to a
//! Subscription's `notification.endpoint.uri`.
//!
//! `tests/distop/reg/csr_health_fields.rs` owns the rule that a client cannot
//! set these members, and the dynamic transitions on the multi-broker topology;
//! this module asserts the absence, the counting and the spelling on the base
//! tier.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The five read-only members Table 5.2.9-2 adds to a registration.
const HEALTH_MEMBERS: [&str; 5] = ["status", "timesSent", "timesFailed", "lastSuccess", "lastFailure"];

/// A registration covering one entity's `speed`, forwarding retrievals to
/// `endpoint`.
fn registration_for(id: &str, entity_id: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": "exclusive",
        "operations": ["retrieveEntity"],
        "information": [{"entities": [{"id": entity_id, "type": "Vehicle"}], "propertyNames": ["speed"]}],
        "@context": [CORE_CONTEXT]
    })
}

/// Read one registration back.
async fn stored_registration(client: &TestClient, id: &str) -> Value {
    let retrieved = client.retrieve_registration(id, Some(CORE_CONTEXT)).await.expect("retrieve registration");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    retrieved.json_value().expect("a retrieved registration is JSON")
}

/// Create one registration and assert the broker stored it.
async fn create_registration(client: &TestClient, body: &Value) {
    let created: TestResponse = client.create_registration(body).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Drive one forwarded retrieval through the registration, whatever it answers.
async fn forward_one_retrieval(client: &TestClient, entity_id: &str) {
    client
        .get_entity(entity_id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("forwarded retrieve");
}

/// Assert exactly the named members are present and the rest are absent.
fn assert_health_members(stored: &Value, present: &[&str], scenario: &str) {
    for member in HEALTH_MEMBERS {
        let expected_present = present.contains(&member);
        assert_eq!(
            stored.get(member).is_some(),
            expected_present,
            "{scenario}: '{member}' presence (Table 5.2.9-2 gives every one cardinality 0..1): {stored}"
        );
    }
}

/// Table 5.2.9-2 defines `status` by "the last attempt to perform a distributed
/// operation" and pairs `timesSent` with a `lastSuccess` "created on first
/// successful operation". A registration that has triggered nothing therefore
/// reports none of the five, on retrieval and on query alike.
#[tokio::test]
async fn regression_registration_health_a_fresh_registration_reports_nothing_observed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("registration:health-fresh");
    let entity_id = scope.id("Vehicle:health-fresh");

    create_registration(client, &registration_for(&id, &entity_id, "http://localhost:9999/source")).await;

    let stored = stored_registration(client, &id).await;
    assert_health_members(&stored, &[], "a registration that has forwarded nothing");

    let queried = client
        .query_registrations(&format!("type=Vehicle&id={entity_id}"), Some(CORE_CONTEXT))
        .await
        .expect("query registrations");
    assert_eq!(queried.status, StatusCode::OK, "query: {:?}", queried.body);
    let results = queried.json_value().expect("a registration query answers JSON");
    let listed = results
        .as_array()
        .expect("a registration query answers an array")
        .iter()
        .find(|candidate| candidate["id"] == json!(id))
        .expect("the registration is among the results");
    assert_health_members(listed, &[], "the same registration in a query result");

    scope.finish().await.expect("cleanup failed");
}

/// After one forwarded operation the source answered, Table 5.2.9-2 has the
/// registration report `status` "ok", the `timesSent` that counts it and the
/// `lastSuccess` created by it — and still nothing about failures.
#[tokio::test]
async fn regression_registration_health_a_successful_forward_reports_ok_and_one_send() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let id = scope.id("registration:health-success");
    let entity_id = scope.id("Vehicle:health-success");
    source
        .set_stub_with_body(
            Method::GET,
            &format!("/ngsi-ld/v1/entities/{entity_id}"),
            200,
            &json!({"id": entity_id, "type": "Vehicle", "speed": {"type": "Property", "value": 42}}),
        )
        .await;

    create_registration(client, &registration_for(&id, &entity_id, &source.url(""))).await;
    forward_one_retrieval(client, &entity_id).await;

    let stored = stored_registration(client, &id).await;
    assert_health_members(&stored, &["status", "timesSent", "lastSuccess"], "after one successful forward");
    assert_eq!(stored["status"], json!("ok"), "the last attempt succeeded: {stored}");
    assert_eq!(stored["timesSent"], json!(1), "one operation was triggered: {stored}");

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// The mirror case: an operation the source refused reports `status` "failed",
/// the `timesFailed` that counts it and the `lastFailure` it happened at.
/// `timesSent` counts it too — Table 5.2.9-2 defines that member as the
/// operations triggered "including failed attempts".
#[tokio::test]
async fn regression_registration_health_a_failed_forward_reports_failed_and_one_failure() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let id = scope.id("registration:health-failure");
    let entity_id = scope.id("Vehicle:health-failure");
    source.set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 502).await;

    create_registration(client, &registration_for(&id, &entity_id, &source.url(""))).await;
    forward_one_retrieval(client, &entity_id).await;

    let stored = stored_registration(client, &id).await;
    assert_eq!(stored["status"], json!("failed"), "the last attempt failed: {stored}");
    assert_eq!(stored["timesFailed"], json!(1), "one operation failed: {stored}");
    assert!(stored.get("lastFailure").is_some(), "the failure is dated: {stored}");
    assert!(
        stored.get("lastSuccess").is_none(),
        "lastSuccess is created on the first successful operation, and there has not been one: {stored}"
    );

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// `status` follows the last attempt rather than accumulating, so a success
/// after a failure reports "ok" while both counters and both timestamps stand.
#[tokio::test]
async fn regression_registration_health_a_success_after_a_failure_flips_the_status_back() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let id = scope.id("registration:health-recovery");
    let entity_id = scope.id("Vehicle:health-recovery");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}");

    // The adapter answers one queued response per request and repeats the last
    // one, so both outcomes are queued before either forward rather than
    // swapped between them: enqueueing after the first request would leave the
    // refusal at the head of the queue and fail the second forward too.
    source.set_stub(Method::GET, &path, 502).await;
    source
        .set_stub_with_body(
            Method::GET,
            &path,
            200,
            &json!({"id": entity_id, "type": "Vehicle", "speed": {"type": "Property", "value": 42}}),
        )
        .await;

    create_registration(client, &registration_for(&id, &entity_id, &source.url(""))).await;
    forward_one_retrieval(client, &entity_id).await;
    let failed = stored_registration(client, &id).await;
    assert_eq!(failed["status"], json!("failed"), "the first attempt failed: {failed}");

    forward_one_retrieval(client, &entity_id).await;

    let stored = stored_registration(client, &id).await;
    assert_eq!(stored["status"], json!("ok"), "the last attempt decides the status: {stored}");
    assert_eq!(stored["timesFailed"], json!(1), "the earlier failure is still counted: {stored}");
    assert!(stored.get("lastFailure").is_some(), "and still dated: {stored}");
    assert!(stored.get("lastSuccess").is_some(), "the success created lastSuccess: {stored}");

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.2 has the broker serve a Context Producer's members back as they
/// were supplied. Table 5.2.9-1 restricts `endpoint` to a dereferenceable URI
/// and Table 5.2.9-1's `jsonldContext` likewise, but neither admits a canonical
/// rewrite: an authority-only URI came back with the empty path a `url::Url`
/// appends, which is a different string from the one the client registered.
#[tokio::test]
async fn regression_registration_health_an_endpoint_round_trips_the_spelling_it_was_written_with() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (index, endpoint) in [
        "http://my.csource.org:1026",
        "http://my.csource.org:1026/",
        "http://my.csource.org:1026/broker1",
        "https://my.csource.org/ngsi-ld/v1",
    ]
    .into_iter()
    .enumerate()
    {
        let id = scope.id(&format!("registration:endpoint-{index}"));
        let entity_id = scope.id(&format!("Vehicle:endpoint-{index}"));
        let mut body = registration_for(&id, &entity_id, endpoint);
        body["jsonldContext"] = json!(endpoint);
        create_registration(client, &body).await;

        let stored = stored_registration(client, &id).await;
        assert_eq!(stored["endpoint"], json!(endpoint), "the endpoint is served back as written: {stored}");
        assert_eq!(
            stored["jsonldContext"],
            json!(endpoint),
            "and so is jsonldContext, which is a URI on the same terms: {stored}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.15-1 restricts a Subscription's notification endpoint `uri` to a
/// dereferenceable URI on the same terms, so it keeps its spelling too — and
/// Table 5.2.14.2-1 gives `status`, `timesSent` and `timesFailed` the same
/// 0..1 cardinality, so a subscription that has notified nothing reports none
/// of them.
#[tokio::test]
async fn regression_registration_health_a_subscription_endpoint_and_counters_match_the_registration_rule() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (index, uri) in ["http://receiver.example.com:8080", "http://receiver.example.com:8080/notify"]
        .into_iter()
        .enumerate()
    {
        let id = scope.id(&format!("subscription:endpoint-{index}"));
        let body = json!({
            "id": id,
            "type": "Subscription",
            "entities": [{"type": "Vehicle", "idPattern": format!("^{}:", scope.id_prefix())}],
            "notification": {"endpoint": {"uri": uri, "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        });
        let created = client.create_subscription(&body).await.expect("create subscription");
        assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

        let retrieved = client.retrieve_subscription(&id, Some(CORE_CONTEXT)).await.expect("retrieve subscription");
        assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
        let stored = retrieved.json_value().expect("a retrieved subscription is JSON");

        assert_eq!(
            stored["notification"]["endpoint"]["uri"],
            json!(uri),
            "the notification endpoint is served back as written: {stored}"
        );
        for member in ["status", "timesSent", "timesFailed"] {
            assert!(
                stored["notification"].get(member).is_none(),
                "nothing has been notified, so '{member}' is absent (Table 5.2.14.2-1): {stored}"
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}
