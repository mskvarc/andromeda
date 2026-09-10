//! Subscription and Endpoint members the broker accepted and then ignored.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.12-1 gives a Subscription an `expandValues`
//! and a `jsonKeys` member, Table 5.2.15-1 gives its `Endpoint` a `cooldown` and
//! a `timeout`, and Table 5.2.14.2-1 gives its `NotificationParams` an
//! output-only `timesFailed`. All five were accepted with `201 Created` and had
//! no effect at all: the two `Endpoint` members did not even survive a retrieve,
//! and a subscription carrying `expandValues` matched exactly as one without it.
//!
//! Two rules about how those members are read back are asserted here too. Table
//! 5.2.14.2-1 restricts `timesSent` and `timesFailed` to "Greater than 0" at
//! cardinality `0..1`, so a counter the system has not moved is an absent
//! member rather than a zero; and Table 5.2.12-1 defines `expandValues` as a
//! coercion applied "prior to executing the query", so the `q` a Subscription
//! echoes is the one its client wrote.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, CapturedRequest, MockResponse, RequestMatcher, ResponseGate},
    jsonld_context_server::JsonLdContextServer,
    notification_server::NotificationServer,
    server_config::MockServerConfig,
};
use axum::body::Bytes;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::{Instant, sleep};
use uuid::Uuid;

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// How long the compiled-subscription cache is given to observe a create.
const CACHE_SETTLE: Duration = Duration::from_millis(400);

/// How long a notification that will arrive is waited for.
const NOTIFICATION_WAIT: Duration = Duration::from_secs(10);

/// How long a second, contrasting subscription is given to fire before the set
/// of notified subscriptions is read.
const CONTRAST_SETTLE: Duration = Duration::from_secs(3);

/// Bind an isolated notification receiver that accepts every delivery.
async fn notification_receiver() -> NotificationServer {
    NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start")
}

/// Bind a callback server and one notification path whose responses the test
/// programmes itself.
async fn programmable_receiver(label: &str) -> (CallbackServer, CallbackPath, String) {
    let server = CallbackServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("callback server must start");
    let path = CallbackPath::parse(format!("/{label}-{}", Uuid::new_v4())).expect("absolute callback path");
    let endpoint = server.endpoint(&path).expect("callback endpoint resolves").to_string();
    (server, path, endpoint)
}

/// Create a subscription, assert it was accepted, and let the matching cache
/// observe it.
async fn create_subscription(client: &TestClient, body: &Value) {
    let response = client.create_subscription(body).await.expect("create subscription request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
    sleep(CACHE_SETTLE).await;
}

/// Retrieve one subscription and decode it.
async fn retrieve_subscription(client: &TestClient, id: &str, context: Option<&str>) -> Value {
    let response = client.retrieve_subscription(id, context).await.expect("retrieve subscription request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.json_value().expect("a retrieved subscription is JSON")
}

/// Assert a refusal is the `BadRequestData` of clauses 5.5.3 and 5.5.4.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// The `subscriptionId` a captured notification names.
fn notified_subscription(captured: &CapturedRequest) -> Option<String> {
    captured
        .json::<Value>()
        .ok()
        .and_then(|body| body["subscriptionId"].as_str().map(str::to_owned))
}

/// The subscriptions the receiver was notified for.
///
/// The first notification is awaited, then a settling window lets any other
/// subscription that would also have fired arrive before the set is read, so a
/// subscription that must *not* fire is given a real chance to.
async fn notified_subscriptions(receiver: &NotificationServer) -> BTreeSet<String> {
    let mut notified = BTreeSet::new();
    if let Ok(first) = receiver.next_request(NOTIFICATION_WAIT).await {
        notified.extend(notified_subscription(&first));
    }
    sleep(CONTRAST_SETTLE).await;
    for captured in receiver.drain_requests().await {
        notified.extend(notified_subscription(&captured));
    }
    notified
}

/// A subscription selecting exactly one entity, notified to `endpoint`.
fn subscription_for(id: &str, entity_id: &str, entity_type: &str, endpoint: &str, context: &Value) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": entity_type}],
        "notification": {"endpoint": {"uri": endpoint}},
        "@context": context
    })
}

/// Append one attribute so the entity changes and its subscriptions are
/// evaluated (clause 5.8.6, `attributeCreated`).
async fn touch_entity(client: &TestClient, entity_id: &str, marker: &str, context: &Value) {
    let fragment = json!({"note": {"type": "Property", "value": marker}, "@context": context});
    let response = client.append_entity_attrs(entity_id, &fragment).await.expect("append attribute request");
    assert!(
        response.status == StatusCode::NO_CONTENT || response.status == StatusCode::CREATED,
        "appending an attribute must succeed: {} {:?}",
        response.status,
        response.body
    );
}

/// Poll a subscription until its notification runtime satisfies `condition`, or
/// the deadline passes; the last document read is returned either way.
async fn poll_notification_state(client: &TestClient, id: &str, within: Duration, condition: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + within;
    let mut latest = retrieve_subscription(client, id, None).await;
    while Instant::now() < deadline {
        if condition(&latest["notification"]) {
            return latest;
        }
        sleep(Duration::from_millis(250)).await;
        latest = retrieve_subscription(client, id, None).await;
    }
    latest
}

/// Table 5.2.12-1: `expandValues` and `jsonKeys` are each a `String` holding a
/// "comma separated list of attribute names". The broker accepted both and
/// dropped them, so a retrieve reported a subscription that carried neither.
///
/// The same table says of `expandValues` that the values it identifies "should
/// be expanded against the supplied @context using JSON-LD type coercion prior
/// to executing the query". That coercion belongs to the filter a match runs,
/// not to the Subscription resource, so the `q` a retrieve echoes is the one
/// the client wrote -- operands included.
#[tokio::test]
async fn regression_subscription_endpoint_members_expand_values_and_json_keys_survive_a_retrieve() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);

    let subscription_id = scope.id("Subscription:opt-in-round-trip");
    let mut body = subscription_for(
        &subscription_id,
        &scope.id("Building:opt-in-round-trip"),
        "Building",
        "http://127.0.0.1:9/notify",
        &context,
    );
    body["q"] = json!("category==\"commercial\";address[city]==\"Berlin\"");
    body["expandValues"] = json!("category");
    body["jsonKeys"] = json!("address");
    create_subscription(client, &body).await;

    let retrieved = retrieve_subscription(client, &subscription_id, Some(TEST_SUITE_CONTEXT.as_str())).await;

    assert_eq!(retrieved["expandValues"], json!("category"), "{retrieved}");
    assert_eq!(retrieved["jsonKeys"], json!("address"), "{retrieved}");
    assert_eq!(
        retrieved["q"],
        json!("category==\"commercial\";address[city]==\"Berlin\""),
        "the echoed filter is the client's own text, with no `expandValues` coercion baked into it: {retrieved}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.15-1: an `Endpoint` carries a `cooldown` and a `timeout`, both
/// numbers of milliseconds. Neither survived a retrieve, because the stored
/// notification parameters were decoded into a type that had no member for
/// either.
#[tokio::test]
async fn regression_subscription_endpoint_members_cooldown_and_timeout_survive_a_retrieve() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);

    let subscription_id = scope.id("Subscription:endpoint-round-trip");
    let mut body = subscription_for(
        &subscription_id,
        &scope.id("Building:endpoint-round-trip"),
        "Building",
        "http://127.0.0.1:9/notify",
        &context,
    );
    body["notification"]["endpoint"]["cooldown"] = json!(2500);
    body["notification"]["endpoint"]["timeout"] = json!(750);
    create_subscription(client, &body).await;

    let retrieved = retrieve_subscription(client, &subscription_id, None).await;

    assert_eq!(retrieved["notification"]["endpoint"]["cooldown"], json!(2500), "{retrieved}");
    assert_eq!(retrieved["notification"]["endpoint"]["timeout"], json!(750), "{retrieved}");

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.15-1 restricts both `cooldown` and `timeout` to "Greater than 0",
/// so zero, a negative number and a non-number are each `BadRequestData`
/// (clause 5.5.4) rather than a silently ignored member.
#[tokio::test]
async fn regression_subscription_endpoint_members_a_non_positive_cooldown_or_timeout_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);

    for (member, value, scenario) in [
        ("cooldown", json!(0), "Table 5.2.15-1 restricts cooldown to greater than 0"),
        ("cooldown", json!("abc"), "Table 5.2.15-1 types cooldown as a Number"),
        ("timeout", json!(-1), "Table 5.2.15-1 restricts timeout to greater than 0"),
        ("timeout", json!(0), "Table 5.2.15-1 restricts timeout to greater than 0"),
    ] {
        let mut body = subscription_for(
            &scope.id(&format!("Subscription:refused-{member}-{}", Uuid::new_v4().simple())),
            &scope.id("Building:refused-endpoint-member"),
            "Building",
            "http://127.0.0.1:9/notify",
            &context,
        );
        body["notification"]["endpoint"][member] = value;

        let response = client.create_subscription(&body).await.expect("create subscription request");

        assert_bad_request_data(&response, scenario);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1: the values of the attributes `expandValues` identifies
/// "should be expanded against the supplied @context using JSON-LD type
/// coercion prior to executing the query". Two subscriptions differing only in
/// that member therefore match differently: the entity's stored value is the
/// IRI the `@vocab`-coerced term expands to, which only the coerced filter
/// compares equal to.
#[tokio::test]
async fn regression_subscription_endpoint_members_expand_values_decides_whether_a_subscription_fires() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");

    let context_server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let context_path = CallbackPath::parse(format!("/expand-values-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let context_url = context_server
        .serve(
            context_path,
            Bytes::from_static(
                br#"{"@context":{"Device":"https://example.org/Device","status":{"@id":"https://example.org/status","@type":"@vocab"},"active":"https://example.org/state/active","note":"https://example.org/note"}}"#,
            ),
            HeaderMap::new(),
        )
        .await
        .expect("context route must be configured");
    let context = json!([context_url]);

    let entity_id = scope.id("Device:expand-values-match");
    let entity = json!({
        "id": entity_id,
        "type": "Device",
        "status": {"type": "Property", "value": "https://example.org/state/active"},
        "@context": context
    });
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let coerced_id = scope.id("Subscription:expand-values-coerced");
    let mut coerced = subscription_for(&coerced_id, &entity_id, "Device", &endpoint, &context);
    coerced["q"] = json!("status==\"active\"");
    coerced["expandValues"] = json!("status");
    create_subscription(client, &coerced).await;

    let literal_id = scope.id("Subscription:expand-values-literal");
    let mut literal = subscription_for(&literal_id, &entity_id, "Device", &endpoint, &context);
    literal["q"] = json!("status==\"active\"");
    create_subscription(client, &literal).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "expand-values", &context).await;

    let notified = notified_subscriptions(&receiver).await;

    assert_eq!(
        notified,
        BTreeSet::from([coerced_id]),
        "only the subscription that opted into value coercion may fire"
    );

    context_server.shutdown().await.expect("context server shutdown failed");
    receiver.shutdown().await.expect("notification receiver shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1: the values of the attributes `jsonKeys` identifies "are to be
/// considered uninterpretable as JSON-LD and should not be expanded against the
/// supplied @context using JSON-LD type coercion prior to executing the query",
/// and clause 4.9 EXAMPLE 11 shows the effect on the `[...]` member the filter
/// addresses. A structured Property value is expanded by JSON-LD into IRI-keyed
/// members, so `address[city]` reaches it only while the member name is
/// expanded; declaring `address` under `jsonKeys` suppresses that expansion and
/// the filter no longer matches. The two subscriptions differ in nothing else.
#[tokio::test]
async fn regression_subscription_endpoint_members_json_keys_decides_whether_a_subscription_fires() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Building:json-keys-match");
    let entity = json!({
        "id": entity_id,
        "type": "Building",
        "address": {"type": "Property", "value": {"city": "Berlin"}},
        "@context": context
    });
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let expanded_id = scope.id("Subscription:json-keys-absent");
    let mut expanded = subscription_for(&expanded_id, &entity_id, "Building", &endpoint, &context);
    expanded["q"] = json!("address[city]==\"Berlin\"");
    create_subscription(client, &expanded).await;

    let literal_id = scope.id("Subscription:json-keys-declared");
    let mut literal = subscription_for(&literal_id, &entity_id, "Building", &endpoint, &context);
    literal["q"] = json!("address[city]==\"Berlin\"");
    literal["jsonKeys"] = json!("address");
    create_subscription(client, &literal).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "json-keys", &context).await;

    let notified = notified_subscriptions(&receiver).await;

    assert_eq!(
        notified,
        BTreeSet::from([expanded_id]),
        "declaring the attribute under jsonKeys must stop the member name from being expanded"
    );

    receiver.shutdown().await.expect("notification receiver shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.15-1: `cooldown` is the "minimum period of time in milliseconds
/// which shall elapse before attempting to make a subsequent notification to the
/// same endpoint after failure", and "if requests are received before the
/// cooldown period has expired, no notification is sent". The second change is
/// therefore swallowed entirely, and a change after the window is notified
/// again.
#[tokio::test]
async fn regression_subscription_endpoint_members_cooldown_suppresses_a_notification_after_a_failure() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);
    let (receiver, path, endpoint) = programmable_receiver("cooldown").await;
    receiver
        .enqueue(
            RequestMatcher::new(Method::POST, path),
            MockResponse::builder().status(StatusCode::INTERNAL_SERVER_ERROR).build(),
        )
        .await;

    let entity_id = scope.id("Building:cooldown");
    let entity = json!({"id": entity_id, "type": "Building", "@context": context});
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let subscription_id = scope.id("Subscription:cooldown");
    let cooldown = Duration::from_secs(8);
    let mut body = subscription_for(&subscription_id, &entity_id, "Building", &endpoint, &context);
    body["notification"]["endpoint"]["cooldown"] = json!(8000);
    create_subscription(client, &body).await;

    touch_entity(client, &entity_id, "first", &context).await;
    receiver
        .next_request(NOTIFICATION_WAIT)
        .await
        .expect("the first notification reaches the endpoint and is refused");
    let failed_at = Instant::now();

    touch_entity(client, &entity_id, "second", &context).await;
    assert!(
        receiver.next_request(Duration::from_secs(4)).await.is_err(),
        "no notification may be sent while the cooldown window is open"
    );

    sleep(cooldown.saturating_sub(failed_at.elapsed()) + Duration::from_millis(500)).await;
    touch_entity(client, &entity_id, "third", &context).await;
    receiver
        .next_request(NOTIFICATION_WAIT)
        .await
        .expect("a notification is sent again once the cooldown window has elapsed");

    receiver.shutdown().await.expect("callback server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.15-1: `timeout` is the "maximum period of time in milliseconds
/// which may elapse before a notification is assumed to have failed". A receiver
/// that never answers therefore fails this subscription's delivery within its
/// own deadline, far sooner than the broker-wide one the delivery would
/// otherwise run under.
#[tokio::test]
async fn regression_subscription_endpoint_members_an_endpoint_timeout_records_a_failed_delivery() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);
    let (receiver, path, endpoint) = programmable_receiver("timeout").await;
    let gate = ResponseGate::closed();
    receiver
        .enqueue(
            RequestMatcher::new(Method::POST, path),
            MockResponse::builder()
                .status(StatusCode::NO_CONTENT)
                .gate(Some(gate.clone())) // clone: the test holds the release handle while the response holds its waiter
                .build(),
        )
        .await;

    let entity_id = scope.id("Building:endpoint-timeout");
    let entity = json!({"id": entity_id, "type": "Building", "@context": context});
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let subscription_id = scope.id("Subscription:endpoint-timeout");
    let mut body = subscription_for(&subscription_id, &entity_id, "Building", &endpoint, &context);
    body["notification"]["endpoint"]["timeout"] = json!(300);
    create_subscription(client, &body).await;

    touch_entity(client, &entity_id, "endpoint-timeout", &context).await;
    receiver
        .next_request(NOTIFICATION_WAIT)
        .await
        .expect("the notification reaches the endpoint, which never answers");

    // The broker-wide deadline is measured in seconds, so a failure recorded
    // this quickly can only be the endpoint's own 300 ms one.
    let latest = poll_notification_state(client, &subscription_id, Duration::from_secs(5), |notification| {
        notification["status"] == json!("failed")
    })
    .await;

    assert_eq!(latest["notification"]["status"], json!("failed"), "{latest}");
    assert!(latest["notification"]["lastFailure"].is_string(), "{latest}");

    gate.release();
    receiver.shutdown().await.expect("callback server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.14.2-1: `timesFailed` is the "number of times an unsuccessful
/// response (or timeout) has been received when delivering the notification",
/// "provided by the system when querying the details of a subscription". Being
/// output only, a value a client supplies at create time is ignored, and the
/// counter the system keeps is what a retrieve reports.
///
/// The same table restricts the member to "Greater than 0" at cardinality
/// `0..1`, so a counter the system has not moved is an absent member and never
/// a `0`. The stored column still floors at zero -- a counter has to be
/// somewhere before it is first advanced -- and the two are different
/// statements about different things.
#[tokio::test]
async fn regression_subscription_endpoint_members_times_failed_is_system_provided_and_counts_failures() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);
    let (receiver, path, endpoint) = programmable_receiver("times-failed").await;
    receiver
        .enqueue(
            RequestMatcher::new(Method::POST, path),
            MockResponse::builder().status(StatusCode::INTERNAL_SERVER_ERROR).build(),
        )
        .await;

    let entity_id = scope.id("Building:times-failed");
    let entity = json!({"id": entity_id, "type": "Building", "@context": context});
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let subscription_id = scope.id("Subscription:times-failed");
    let mut body = subscription_for(&subscription_id, &entity_id, "Building", &endpoint, &context);
    body["notification"]["timesFailed"] = json!(99);
    create_subscription(client, &body).await;

    let fresh = retrieve_subscription(client, &subscription_id, None).await;
    assert!(
        fresh["notification"].get("timesFailed").is_none(),
        "Table 5.2.14.2-1 restricts timesFailed to \"Greater than 0\" at cardinality 0..1, so an unmoved counter is absent: {fresh}"
    );
    assert!(
        fresh["notification"].get("timesSent").is_none(),
        "the same holds of timesSent, which the same table restricts the same way: {fresh}"
    );

    touch_entity(client, &entity_id, "times-failed", &context).await;
    receiver
        .next_request(NOTIFICATION_WAIT)
        .await
        .expect("the notification reaches the endpoint and is refused");

    let latest = poll_notification_state(client, &subscription_id, Duration::from_secs(10), |notification| {
        notification["timesFailed"].as_i64().is_some_and(|failures| failures >= 1)
    })
    .await;

    assert!(
        latest["notification"]["timesFailed"].as_i64().is_some_and(|failures| failures >= 1),
        "a refused delivery must advance timesFailed: {latest}"
    );
    assert!(
        latest["notification"]["timesSent"].as_i64().is_some_and(|sent| sent >= 1),
        "the attempt was counted when the slot was claimed: {latest}"
    );
    assert_eq!(latest["notification"]["status"], json!("failed"), "{latest}");

    receiver.shutdown().await.expect("callback server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
