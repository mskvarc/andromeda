//! A Property whose value is the empty array, through the Temporal API, a
//! notification and a `q` filter.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.5-1 makes `value` "Any JSON value as
//! defined by IETF RFC 8259" and clause 3.1 lists "an array" among the
//! NGSI-LD Values. A Temporal Evolution records the empty array as the
//! observation it is (clauses 5.6.11, 5.6.12, 4.5.7 and 4.5.9), a notification
//! carries it as the current value (clause 5.8.6), and `q` reads it as clause
//! 4.9 reads any array: the attribute exists, and no operand is included in it.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The instant the emptied observation is made at.
const EMPTIED_AT: &str = "2024-01-02T00:00:00Z";

/// How long a notification that is owed is waited for.
const ARRIVAL: Duration = Duration::from_secs(10);

/// Create one entity, asserting clause 5.6.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let created = client.post_entity(body).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create {}: {:?}", body["id"], created.body);
}

/// The Temporal Evolution of `id` in the representation `query` selects.
async fn temporal(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_temporal_entity(id, query, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval of {id}: {:?}", response.body);
    response.json_value().expect("a temporal answer is JSON")
}

/// Clause 5.6.12: Temporal Add Attributes records an observation of the empty
/// array, which both temporal representations answer as written.
#[tokio::test]
async fn regression_r05_temporal_add_attributes_records_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:temporal-add");
    let evolution = json!({
        "id": id,
        "type": "EmptyProbe",
        "speed": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}],
        "@context": CORE_CONTEXT
    });
    let created = client.create_temporal_entity(&evolution).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let added = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(&id)))
        .header("content-type", "application/ld+json")
        .json(&json!({
            "tags": [{"type": "Property", "value": [], "observedAt": EMPTIED_AT}],
            "@context": CORE_CONTEXT
        }))
        .send()
        .await
        .expect("temporal add attributes request");
    assert_eq!(added.status, StatusCode::NO_CONTENT, "{:?}", added.body);

    let retrieved = temporal(client, &id, WINDOW).await;
    assert_eq!(retrieved["tags"][0]["type"], json!("Property"), "{retrieved}");
    assert_eq!(retrieved["tags"][0]["value"], json!([]), "{retrieved}");
    let simplified = temporal(client, &id, &format!("{WINDOW}&format=temporalValues")).await;
    assert_eq!(simplified["tags"]["values"][0], json!([[], EMPTIED_AT]), "{simplified}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.8.6: a notification carries the empty array as the value the
/// change left.
#[tokio::test]
async fn regression_r05_a_notification_carries_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let id = scope.id("EmptyProbe:notified");
    let subscription_id = scope.id("Subscription:emptied");
    create(
        client,
        &json!({"id": id, "type": "EmptyProbe", "tags": {"type": "Property", "value": ["a", "b"]}, "@context": [CORE_CONTEXT]}),
    )
    .await;
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"id": id, "type": "EmptyProbe"}],
            "watchedAttributes": ["tags"],
            "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url(), "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);

    let updated = client
        .update_entity_attrs(&id, &json!({"tags": {"type": "Property", "value": []}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("update attributes request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_eq!(
        notification.body["data"][0]["tags"],
        json!({"type": "Property", "value": []}),
        "{}",
        notification.body
    );

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.9: an attribute holding the empty array exists, so the existence
/// term selects it, and no operand is included in it, so an equality term does
/// not.
#[tokio::test]
async fn regression_r05_q_finds_the_empty_array_by_existence_but_never_by_equality() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let emptied = scope.id("EmptyProbe:q-emptied");
    let tagged = scope.id("EmptyProbe:q-tagged");
    for (id, tags) in [(&emptied, json!([])), (&tagged, json!(["a"]))] {
        create(
            client,
            &json!({"id": id, "type": "EmptyProbe", "tags": {"type": "Property", "value": tags}, "@context": [CORE_CONTEXT]}),
        )
        .await;
    }
    let ids = format!("{},{}", urlencoding::encode(&emptied), urlencoding::encode(&tagged));

    let matching = |q: &'static str| {
        let query = format!("type=EmptyProbe&id={ids}&q={}", urlencoding::encode(q));
        async move {
            let response = client
                .query_entities(&query, "application/json", Some(CORE_CONTEXT))
                .await
                .expect("query entities request");
            assert_eq!(response.status, StatusCode::OK, "{q}: {:?}", response.body);
            let mut found: Vec<String> = response
                .json_value()
                .expect("a query answer")
                .as_array()
                .expect("an array of entities")
                .iter()
                .map(|entity| entity["id"].as_str().expect("an entity id").to_owned())
                .collect();
            found.sort_unstable();
            found
        }
    };

    let mut both = vec![emptied, tagged.clone()]; // clone: the second expectation names the tagged entity alone
    both.sort_unstable();
    assert_eq!(matching("tags").await, both);
    assert_eq!(matching("tags==\"a\"").await, vec![tagged]);

    scope.finish().await.expect("environment cleanup");
}
