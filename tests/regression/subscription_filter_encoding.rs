//! Whether a `q` filter carried in a JSON body is percent-decoded.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.9 defines the `q` member as "A valid query
//! string as per clause 4.9", and clause 4.9 names percent-encoding exactly
//! once: "In case of HTTP binding, whenever the string acting as a filter is
//! part of the HTTP binding's URI, then it shall be URI-encoded
//! (percent-encoded, as described in IETF RFC 3986 [5])." Percent-encoding is
//! therefore a property of the URI transport alone. A `q` conveyed in a JSON
//! body -- a Subscription (clause 5.2.12), a POST Query (clause 5.2.23) -- is
//! not part of any URI, so it is the raw query-language string and a `%20`
//! within a quoted operand stands for the two-character sequence `%` `2` `0`,
//! not for a space.
//!
//! This is the same rule the operand scanner already documents: a quoted
//! operand is the clause 4.9 `quotedStr`, an IETF RFC 8259 JSON String, whose
//! grammar has no percent-decoding step. Decoding it would rewrite
//! `ref=="urn:x:a%2Fb"` into `ref=="urn:x:a/b"` and silently change the meaning
//! of a stored subscription whose operand is a URN that legitimately carries
//! `%2F`. The URI binding reaches the same operand value by a different route:
//! `serde_urlencoded` form-decodes the whole query string once before the
//! filter is parsed, which is where a URI-borne `%20` becomes a space.
//!
//! These tests pin that a body-borne operand is matched byte for byte: a
//! literal space matches a value that has one, and a `%20` matches only a value
//! that literally contains `%20`. The behaviour is spec-correct; the ETSI case
//! `046_05_01` subscribes with `name=="Eiffel%20Tower"` in a JSON body against an
//! entity named `Eiffel Tower`, which cannot match, and is corrected against
//! the specification in the test-material patch set.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// How long a notification that is owed is waited for.
const ARRIVAL: Duration = Duration::from_secs(10);

/// How long a notification that is not owed is waited against.
const SILENCE: Duration = Duration::from_secs(5);

/// A Building whose `name` carries `value`, plus the watched attribute so a
/// later append has something to change.
fn building(id: &str, name_value: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": name_value},
        "airQualityLevel": {"type": "Property", "value": 4},
        "@context": [CORE_CONTEXT]
    })
}

/// A subscription watching `airQualityLevel` and narrowed by `q`, notified over
/// `endpoint`.
fn subscription(id: &str, endpoint: &str, q: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "watchedAttributes": ["airQualityLevel"],
        "q": q,
        "notification": {"format": "normalized", "endpoint": {"uri": endpoint, "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// Create an entity and assert it was taken.
async fn create(client: &TestClient, body: &Value) {
    let created = client.post_entity(body).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);
}

/// Create a subscription and assert it was taken.
async fn subscribe(client: &TestClient, body: &Value) {
    let created = client.create_subscription(body).await.expect("create subscription");
    assert_eq!(created.status, StatusCode::CREATED, "create subscription: {:?}", created.body);
}

/// Change the watched attribute, which is what triggers the notification.
async fn touch(client: &TestClient, entity_id: &str) {
    let updated = client
        .append_entity_attrs(
            entity_id,
            &json!({"airQualityLevel": {"type": "Property", "value": 5}, "@context": [CORE_CONTEXT]}),
        )
        .await
        .expect("append attributes");
    assert!(updated.status.is_success(), "append: {:?}", updated.body);
}

/// Clause 4.9: a body-borne `q` is the raw query-language string, so a quoted
/// operand written with a literal space matches an entity whose value has that
/// space -- the operand is compared as the JSON String it is.
#[tokio::test]
async fn regression_subscription_filter_a_literal_space_in_a_body_q_matches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:body-q-literal-space");
    let subscription_id = scope.id("Subscription:body-q-literal-space");

    create(client, &building(&entity_id, "Eiffel Tower")).await;
    subscribe(client, &subscription(&subscription_id, &receiver.endpoint_url(), "name==\"Eiffel Tower\"")).await;
    receiver.clear_notifications().await;

    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_eq!(notification.body["data"][0]["id"], json!(entity_id), "{}", notification.body);

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: percent-encoding is the URI binding's concern, so a `%20` inside
/// a body-borne operand is the three characters `%20`, not a space. It does not
/// match an entity named `Eiffel Tower`, and no notification is owed.
#[tokio::test]
async fn regression_subscription_filter_a_percent_sequence_in_a_body_q_is_not_decoded() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:body-q-percent-encoded");
    let subscription_id = scope.id("Subscription:body-q-percent-encoded");

    create(client, &building(&entity_id, "Eiffel Tower")).await;
    subscribe(client, &subscription(&subscription_id, &receiver.endpoint_url(), "name==\"Eiffel%20Tower\"")).await;
    receiver.clear_notifications().await;

    touch(client, &entity_id).await;

    receiver.wait_for_no_subscription_notification(&subscription_id, SILENCE).await;

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// The operand is matched byte for byte, not always refused: an entity whose
/// `name` literally contains `%20` does match the same percent-carrying filter,
/// which is what distinguishes "not decoded" from "never matches".
#[tokio::test]
async fn regression_subscription_filter_a_percent_sequence_matches_a_literal_percent_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:body-q-literal-percent");
    let subscription_id = scope.id("Subscription:body-q-literal-percent");

    create(client, &building(&entity_id, "Eiffel%20Tower")).await;
    subscribe(client, &subscription(&subscription_id, &receiver.endpoint_url(), "name==\"Eiffel%20Tower\"")).await;
    receiver.clear_notifications().await;

    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_eq!(notification.body["data"][0]["id"], json!(entity_id), "{}", notification.body);

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}
