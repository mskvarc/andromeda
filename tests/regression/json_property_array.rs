//! A `JsonProperty` whose `json` is an array reads back as that array.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.24.2: `json` is "a raw JSON object (or
//! array of objects) which contains data which is not available for JSON-LD
//! interpretation. This means that the attributes within the JSON object are
//! never subject to JSON-LD term expansion or compaction". The core context
//! types `json` as `@json`, so the array is stored as one JSON literal, and
//! clause 4.6.4 returns "the original content". The compaction walk added the
//! literal's document by the JSON-LD add-value rule, element by element, so a
//! one-element array read back as its element and a nested array was flattened
//! into its parent, on the Core and the Temporal API alike.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

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

/// How long the owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// The first instant a temporal instance is observed at.
const FIRST: &str = "2022-08-09T18:25:02Z";

/// The second instant a temporal instance is observed at.
const SECOND: &str = "2022-08-10T18:25:02Z";

/// A `JsonProperty` holding `json`.
fn json_property(json: &Value) -> Value {
    json!({"type": "JsonProperty", "json": json})
}

/// Create the entity `id` of type `JsonArrayProbe` holding `attributes`.
async fn create(client: &TestClient, id: &str, attributes: &Value) {
    let mut entity = attributes.clone(); // clone: the caller keeps the attributes it compares against
    entity["id"] = json!(id);
    entity["type"] = json!("JsonArrayProbe");
    entity["@context"] = json!(CORE_CONTEXT);
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// The entity `id` in the representation `query` selects.
async fn retrieved(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieval of {id}: {:?}", response.body);
    response.json_value().expect("an entity is JSON")
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

/// Send `body` to `path` with `method` as JSON-LD and expect `204 No Content`.
async fn write(client: &TestClient, method: Method, path: &str, body: &Value) {
    let response = client
        .raw(method, path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("write request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{path}: {:?}", response.body);
}

/// Clauses 4.5.24.2 and 4.5.24.3: a one-element array, an array holding an
/// array, and a sub-attribute's one-element array read back as written in the
/// normalized, the concise and the key-values representation.
#[tokio::test]
async fn regression_r81_a_json_array_reads_back_as_written_in_every_representation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("JsonArrayProbe:read");
    let tickets = json!([{"id": "85a6cc52-0589-45f9", "value": "Overstay 60 minutes"}]);
    let nested = json!([{"a": 1}, [1, 2], "text"]);
    let meta = json!([{"k": "v"}]);
    create(
        client,
        &id,
        &json!({
            "tickets": json_property(&tickets),
            "nested": json_property(&nested),
            "speed": {"type": "Property", "value": 1, "meta": json_property(&meta)}
        }),
    )
    .await;

    let normalized = retrieved(client, &id, "").await;
    let concise = retrieved(client, &id, "options=concise").await;
    let key_values = retrieved(client, &id, "options=keyValues").await;
    scope.finish().await.expect("environment cleanup");

    assert_eq!(normalized["tickets"], json_property(&tickets), "{normalized}");
    assert_eq!(normalized["nested"], json_property(&nested), "{normalized}");
    assert_eq!(normalized["speed"]["meta"], json_property(&meta), "{normalized}");
    assert_eq!(concise["tickets"], json!({"json": tickets}), "{concise}");
    assert_eq!(concise["nested"], json!({"json": nested}), "{concise}");
    assert_eq!(key_values["tickets"], json!({"json": tickets}), "{key_values}");
    assert_eq!(key_values["nested"], json!({"json": nested}), "{key_values}");
}

/// Partial Attribute Update (clause 5.6.4), Append Attributes (5.6.3), Merge
/// Entity (5.6.17) and Replace Attribute (5.6.19) each store a one-element
/// array that reads back as written.
#[tokio::test]
async fn regression_r81_attribute_writes_keep_a_one_element_json_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("JsonArrayProbe:write");
    create(
        client,
        &id,
        &json!({"tickets": json_property(&json!({"id": "a"})), "replaced": json_property(&json!({"id": "x"}))}),
    )
    .await;
    let encoded = urlencoding::encode(&id).into_owned();

    let patched = json!([{"id": "b"}]);
    let appended = json!([{"id": "c"}]);
    let merged = json!([{"id": "d"}]);
    let replaced = json!([{"id": "e"}]);
    let mut fragment = json_property(&patched);
    fragment["@context"] = json!(CORE_CONTEXT);
    write(client, Method::PATCH, &format!("/ngsi-ld/v1/entities/{encoded}/attrs/tickets"), &fragment).await;
    write(
        client,
        Method::POST,
        &format!("/ngsi-ld/v1/entities/{encoded}/attrs"),
        &json!({"appended": json_property(&appended), "@context": CORE_CONTEXT}),
    )
    .await;
    write(
        client,
        Method::PATCH,
        &format!("/ngsi-ld/v1/entities/{encoded}"),
        &json!({"merged": json_property(&merged), "@context": CORE_CONTEXT}),
    )
    .await;
    let mut replacement = json_property(&replaced);
    replacement["@context"] = json!(CORE_CONTEXT);
    write(client, Method::PUT, &format!("/ngsi-ld/v1/entities/{encoded}/attrs/replaced"), &replacement).await;

    let entity = retrieved(client, &id, "").await;
    scope.finish().await.expect("environment cleanup");

    assert_eq!(entity["tickets"], json_property(&patched), "{entity}");
    assert_eq!(entity["appended"], json_property(&appended), "{entity}");
    assert_eq!(entity["merged"], json_property(&merged), "{entity}");
    assert_eq!(entity["replaced"], json_property(&replaced), "{entity}");
}

/// Clauses 5.6.11 and 5.6.12 with clause 4.5.9: a Temporal Evolution keeps a
/// one-element array in its normalized instances and in its `jsons` pairs,
/// whether it was written as instances or as pairs.
#[tokio::test]
async fn regression_r81_a_temporal_evolution_keeps_a_one_element_json_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("JsonArrayProbe:temporal");
    let first = json!([{"id": "a"}]);
    let second = json!([{"id": "b"}]);
    let paired = json!([{"id": "p"}]);
    let evolution = json!({
        "id": id,
        "type": "JsonArrayProbe",
        "tickets": [
            {"type": "JsonProperty", "json": first, "observedAt": FIRST},
            {"type": "JsonProperty", "json": second, "observedAt": SECOND}
        ],
        "@context": CORE_CONTEXT
    });
    let created = client.create_temporal_entity(&evolution).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    write(
        client,
        Method::POST,
        &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(&id)),
        &json!({"pairs": {"type": "JsonProperty", "jsons": [[{"json": paired}, FIRST]]}, "@context": CORE_CONTEXT}),
    )
    .await;

    let normalized = temporal(client, &id, WINDOW).await;
    let simplified = temporal(client, &id, &format!("{WINDOW}&format=temporalValues")).await;
    scope.finish().await.expect("environment cleanup");

    let instances: Vec<(&Value, &Value)> = normalized["tickets"]
        .as_array()
        .expect("the tickets history")
        .iter()
        .map(|instance| (&instance["json"], &instance["observedAt"]))
        .collect();
    assert_eq!(instances, vec![(&first, &json!(FIRST)), (&second, &json!(SECOND))], "{normalized}");
    assert_eq!(
        simplified["tickets"],
        json!({"type": "JsonProperty", "jsons": [[{"json": first}, FIRST], [{"json": second}, SECOND]]}),
        "{simplified}"
    );
    assert_eq!(
        simplified["pairs"],
        json!({"type": "JsonProperty", "jsons": [[{"json": paired}, FIRST]]}),
        "{simplified}"
    );
}

/// Clause 5.8.6 with `showChanges`: a notification carries the one-element
/// array as `json` and the one it replaced as `previousJson` (clause 4.5.24.2:
/// "The representation is the same as that of "json"").
#[tokio::test]
async fn regression_r81_a_notification_keeps_a_one_element_json_array_and_its_previous_json() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let client = scope.client();
    let entity_type = scope.entity_type("JsonArrayNotified");
    let id = scope.id(&format!("{entity_type}:1"));
    let before = json!([{"id": "a"}]);
    let after = json!([{"id": "b"}]);
    let created = client
        .post_entity(&json!({"id": id, "type": entity_type, "tickets": json_property(&before), "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let subscription_id = scope.id(&format!("Subscription:{entity_type}"));
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notificationTrigger": ["attributeUpdated"],
        "notification": {"format": "normalized", "showChanges": true, "endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
        "@context": CORE_CONTEXT
    });
    let subscribed = client.create_subscription(&subscription).await.expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
    let mut fragment = json_property(&after);
    fragment["@context"] = json!(CORE_CONTEXT);
    write(
        client,
        Method::PATCH,
        &format!("/ngsi-ld/v1/entities/{}/attrs/tickets", urlencoding::encode(&id)),
        &fragment,
    )
    .await;

    let notification = adapter.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    adapter.shutdown().await;
    scope.finish().await.expect("environment cleanup");

    let tickets = &notification.body["data"][0]["tickets"];
    assert_eq!(tickets["json"], after, "{tickets}");
    assert_eq!(tickets["previousJson"], before, "{tickets}");
}
