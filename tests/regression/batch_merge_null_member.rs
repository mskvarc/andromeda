//! ETSI GS CIM 009 v1.9.1 clause 5.6.20.4: "For each of the NGSI-LD Entities included in the input Array execute the behaviour defined by clause 5.6.17, but limited to a local operation". Clause 5.5.12: "For each member of the Fragment, whose value is an NGSI-LD Null, contained by the target, the target member is removed." A first-level `"speed": "urn:ngsi-ld:null"` therefore removes the whole attribute, every instance and every `datasetId` with it, through `PATCH /entities/{id}` and Batch Entity Merge alike. A concise Property whose value is the Null, by contrast, deletes the default instance only (clause 5.6.17.4).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The NGSI-LD Null of clause 4.5.0.
const NULL: &str = "urn:ngsi-ld:null";

/// The first named dataset `speed` is held in.
const DATASET_A: &str = "urn:ngsi-ld:Dataset:A";

/// The second named dataset `speed` is held in.
const DATASET_B: &str = "urn:ngsi-ld:Dataset:B";

/// The expanded name of the `settings` term the request context coerces to `@json`.
const SETTINGS: &str = "https://example.org/settings";

/// How long the owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// How long the absence of a further notification is watched for.
const QUIET: Duration = Duration::from_secs(3);

/// The request context: `settings` coerced to `@json`, then the core context.
fn context() -> Value {
    json!([{"settings": {"@id": SETTINGS, "@type": "@json"}}, CORE_CONTEXT])
}

/// Send `body` as `application/ld+json` to the broker path `path`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, &format!("/ngsi-ld/v1/{path}"))
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("write request")
}

/// Create the entity `id` of `entity_type` carrying `attributes`.
async fn create(client: &TestClient, id: &str, entity_type: &str, attributes: &Value) {
    let mut body = json!({"id": id, "type": entity_type, "@context": context()});
    for (name, member) in attributes.as_object().expect("the attributes are an object") {
        body[name] = member.clone(); // clone: the caller creates several entities from one set of attributes
    }
    let created = send(client, Method::POST, "entities", &body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{id}: {:?}", created.body);
}

/// `speed` held as a default, an A and a B instance, beside `brand`.
fn three_speeds() -> Value {
    json!({
        "speed": [
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": DATASET_A},
            {"type": "Property", "value": 3, "datasetId": DATASET_B}
        ],
        "brand": {"type": "Property", "value": "x"}
    })
}

/// Merge `fragment` into `single` through Merge Entity and into `batched`
/// through Batch Entity Merge, asserting both answer 204.
async fn merge_both_ways(client: &TestClient, single: &str, batched: &str, entity_type: &str, fragment: &Value) {
    let mut patch = fragment.clone(); // clone: the fragment is sent two ways
    patch["@context"] = context();
    let merged = send(client, Method::PATCH, &format!("entities/{}", urlencoding::encode(single)), &patch).await;
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "merge entity: {:?}", merged.body);
    let mut entity = patch;
    entity["id"] = json!(batched);
    entity["type"] = json!(entity_type);
    let merged = send(client, Method::POST, "entityOperations/merge", &json!([entity])).await;
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "batch entity merge: {:?}", merged.body);
}

/// The entity `id` as a read serves it, without its `id`.
async fn served(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "{id}: {:?}", response.body);
    let mut entity = response.json_value().expect("an entity body");
    entity.as_object_mut().expect("an entity object").remove("id");
    entity
}

/// Clause 5.5.12: the bare Null removes the target member, so both merges leave the entity without any instance of `speed` and with `brand`.
#[tokio::test]
async fn regression_r06_a_batch_merge_removes_every_instance_of_a_member_set_to_the_null() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NullMember");
    let single = scope.id(&format!("{entity_type}:single"));
    let batched = scope.id(&format!("{entity_type}:batched"));
    create(client, &single, &entity_type, &three_speeds()).await;
    create(client, &batched, &entity_type, &three_speeds()).await;

    merge_both_ways(client, &single, &batched, &entity_type, &json!({"speed": NULL})).await;

    let by_batch = served(client, &batched).await;
    assert!(by_batch.get("speed").is_none(), "every instance of speed is removed: {by_batch}");
    assert_eq!(by_batch["brand"]["value"], json!("x"), "{by_batch}");
    assert_eq!(by_batch, served(client, &single).await, "the batch merge does what Merge Entity does");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.17.4: a reified instance without a `datasetId` targets the default instance, so its Null deletes that one alone, through either merge.
#[tokio::test]
async fn regression_r06_a_batch_merge_of_a_reified_null_deletes_only_the_default_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NullInstance");
    let single = scope.id(&format!("{entity_type}:single"));
    let batched = scope.id(&format!("{entity_type}:batched"));
    create(client, &single, &entity_type, &three_speeds()).await;
    create(client, &batched, &entity_type, &three_speeds()).await;

    merge_both_ways(client, &single, &batched, &entity_type, &json!({"speed": {"type": "Property", "value": NULL}})).await;

    let by_batch = served(client, &batched).await;
    let mut datasets: Vec<&str> = by_batch["speed"]
        .as_array()
        .expect("two instances are an array (clause 4.5.5)")
        .iter()
        .map(|instance| instance["datasetId"].as_str().expect("a named instance"))
        .collect();
    datasets.sort_unstable();
    assert_eq!(datasets, vec![DATASET_A, DATASET_B], "{by_batch}");
    assert_eq!(by_batch, served(client, &single).await, "the batch merge does what Merge Entity does");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.5.12 merges to arbitrary depth: a key of a structured value, a key of a JSON literal and one language are deleted by both merges alike.
#[tokio::test]
async fn regression_r06_a_batch_merge_deletes_a_value_key_a_literal_key_and_a_language_as_merge_entity_does() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NullDeep");
    let single = scope.id(&format!("{entity_type}:single"));
    let batched = scope.id(&format!("{entity_type}:batched"));
    let held = json!({
        "address": {"type": "Property", "value": {"street": "a", "city": "b", "country": "c"}},
        "label": {"type": "LanguageProperty", "languageMap": {"en": "hi", "de": "hallo"}},
        "config": {"type": "Property", "value": {"settings": {"a": 1, "b": 2}}}
    });
    create(client, &single, &entity_type, &held).await;
    create(client, &batched, &entity_type, &held).await;

    let fragment = json!({
        "address": {"type": "Property", "value": {"street": "z", "country": NULL}},
        "label": {"type": "LanguageProperty", "languageMap": {"de": NULL}},
        "config": {"type": "Property", "value": {"settings": {"b": NULL}}}
    });
    merge_both_ways(client, &single, &batched, &entity_type, &fragment).await;

    let by_batch = served(client, &batched).await;
    assert_eq!(by_batch["address"]["value"], json!({"street": "z", "city": "b"}), "{by_batch}");
    assert_eq!(by_batch["label"]["languageMap"], json!({"en": "hi"}), "{by_batch}");
    assert_eq!(by_batch["config"]["value"][SETTINGS]["@value"], json!({"a": 1}), "{by_batch}");
    assert_eq!(by_batch, served(client, &single).await, "the batch merge does what Merge Entity does");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.8.6 notifies a deleted Attribute as NGSI-LD Null, each deleted instance with its own `datasetId`: the batch merge removing `speed` owes one notification carrying all three.
#[tokio::test]
async fn regression_r06_a_batch_merge_notifies_every_removed_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NullNotified");
    let id = scope.id(&format!("{entity_type}:1"));
    create(client, &id, &entity_type, &three_speeds()).await;
    let subscription = scope.id(&format!("Subscription:{entity_type}"));
    let created = client
        .create_subscription(&json!({
            "id": subscription,
            "type": "Subscription",
            "entities": [{"type": entity_type}],
            "watchedAttributes": ["speed"],
            "notificationTrigger": ["attributeDeleted"],
            "notification": {"format": "normalized", "endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let merged = send(
        client,
        Method::POST,
        "entityOperations/merge",
        &json!([{"id": id, "type": entity_type, "speed": NULL, "@context": CORE_CONTEXT}]),
    )
    .await;
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "{:?}", merged.body);

    let notification = adapter.wait_for_subscription_notification(&subscription, ARRIVAL).await;
    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;
    let data = notification.body["data"].as_array().expect("a notification carries data");
    assert_eq!(data.len(), 1, "{}", notification.body);
    let speed = data[0]["speed"].as_array().expect("three deleted instances are an array (clause 4.5.5)");
    let mut datasets: Vec<Option<&str>> = speed.iter().map(|instance| instance["datasetId"].as_str()).collect();
    datasets.sort_unstable();
    assert_eq!(datasets, vec![None, Some(DATASET_A), Some(DATASET_B)], "{}", notification.body);
    assert!(
        speed.iter().all(|instance| instance["value"] == json!(NULL)),
        "every instance is NGSI-LD Null: {}",
        notification.body
    );

    adapter.shutdown().await;
    scope.finish().await.expect("environment cleanup");
}
