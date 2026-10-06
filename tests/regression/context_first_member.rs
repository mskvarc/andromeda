//! `@context` is the first member of an `application/ld+json` response, and a
//! collection carries it on every item.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.6 has a response served as
//! `application/ld+json` carry its `@context` in the body, and clause 4.4 puts
//! the `@context` of a JSON-LD document first. A parsed JSON map does not keep
//! member order, so these cases read the raw response text: it has to open
//! with `{"@context"` for a single resource, and every item of a collection
//! (clauses 5.8.4 and 5.9.4, which answer an array of resources) has to open
//! that way too. The Snapshot half of the family is in the snapshot suite,
//! which owns the client a Snapshot needs.
//!
//! Needs the host broker: the cases drive the HTTP API and are not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::raw_json_text::{array_items, opens_with_context};
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::{str::from_utf8, time::Duration};
use tokio::time::sleep;

/// The raw response text, asserting it is UTF-8.
fn text_of(response: &TestResponse) -> &str {
    from_utf8(&response.body).expect("a JSON-LD response is UTF-8")
}

/// Assert that every item of the collection `response` carries `@context` first,
/// and that the collection has at least `minimum` items.
fn assert_every_item_opens_with_context(response: &TestResponse, minimum: usize, resource: &str) {
    let text = text_of(response);
    let items = array_items(text).expect("a collection is a JSON array");
    assert!(items.len() >= minimum, "{resource} collection holds the created items: {text}");
    for item in items {
        assert!(opens_with_context(item), "a {resource} collection item opens with @context: {item}");
    }
}

/// Create the Subscription the cases read.
async fn create_subscription(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "create subscription: {:?}", created.body);
}

/// Create the Context Source Registration the cases read.
async fn create_registration(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": "http://127.0.0.1:9/source",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&body).await.expect("create registration request");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);
}

/// Clause 6.3.6: a Subscription served as `application/ld+json` opens with its
/// `@context`, and so does every item of the Subscription collection.
#[tokio::test]
async fn regression_context_first_member_subscription_and_its_collection() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = [scope.id("Subscription:context-first-1"), scope.id("Subscription:context-first-2")];
    for id in &ids {
        create_subscription(client, id).await;
    }

    let single = client.retrieve_subscription(&ids[0], None).await.expect("retrieve subscription request");
    assert_eq!(single.status, StatusCode::OK, "retrieve: {:?}", single.body);
    assert!(opens_with_context(text_of(&single)), "a Subscription opens with @context: {}", text_of(&single));

    let collection = client.query_subscriptions("", None).await.expect("query subscriptions request");
    assert_eq!(collection.status, StatusCode::OK, "query: {:?}", collection.body);
    assert_every_item_opens_with_context(&collection, ids.len(), "Subscription");

    for id in &ids {
        client.delete_subscription(id).await.expect("delete subscription request");
    }
    scope.finish().await.expect("environment cleanup");
}

/// Clause 6.3.6: a Context Source Registration served as `application/ld+json`
/// opens with its `@context`, and so does every item of the registration
/// collection.
#[tokio::test]
async fn regression_context_first_member_registration_and_its_collection() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = [
        scope.id("ContextSourceRegistration:context-first-1"),
        scope.id("ContextSourceRegistration:context-first-2"),
    ];
    for id in &ids {
        create_registration(client, id).await;
    }

    let single = client.retrieve_registration(&ids[0], None).await.expect("retrieve registration request");
    assert_eq!(single.status, StatusCode::OK, "retrieve: {:?}", single.body);
    assert!(
        opens_with_context(text_of(&single)),
        "a Context Source Registration opens with @context: {}",
        text_of(&single)
    );

    // Clause 5.10.2.4 refuses a query naming no entity type, attribute, `q` or
    // `georel` as too wide, so the collection is read by the type both
    // registrations cover.
    let collection = client.query_registrations("type=Vehicle", None).await.expect("query registrations request");
    assert_eq!(collection.status, StatusCode::OK, "query: {:?}", collection.body);
    assert_every_item_opens_with_context(&collection, ids.len(), "Context Source Registration");

    for id in &ids {
        client.delete_registration(id).await.expect("delete registration request");
    }
    scope.finish().await.expect("environment cleanup");
}

/// Clause 6.3.6: an `EntityMap` served as `application/ld+json` opens with its
/// `@context`.
#[tokio::test]
async fn regression_context_first_member_entity_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("Vehicle");
    let id = scope.id("Vehicle:context-first-map");
    let entity = json!({"id": id, "type": entity_type, "@context": [CORE_CONTEXT]});
    let created = client.post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);

    let minted = client
        .raw(
            Method::GET,
            &format!("/ngsi-ld/v1/entityMaps?{}", scope.scope_query(&format!("type={entity_type}"))),
        )
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("create EntityMap request");
    assert_eq!(minted.status, StatusCode::CREATED, "create EntityMap: {:?}", minted.body);
    let map_id = minted.json_value().expect("a created EntityMap is JSON")["id"]
        .as_str()
        .expect("the created EntityMap carries an id")
        .to_owned();

    let retrieved = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entityMaps/{}", urlencoding::encode(&map_id)))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("retrieve EntityMap request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve EntityMap: {:?}", retrieved.body);
    assert!(
        opens_with_context(text_of(&retrieved)),
        "an EntityMap opens with @context: {}",
        text_of(&retrieved)
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap request");
    scope.finish().await.expect("environment cleanup");
}

/// Clause 6.3.6: an entity retrieved, an entity query answer and a Temporal
/// Evolution served as `application/ld+json` open with `@context`, the
/// collection on every item.
#[tokio::test]
async fn regression_context_first_member_entity_query_and_temporal_retrieval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("Vehicle");
    let id = scope.id("Vehicle:context-first-entity");
    let entity = json!({
        "id": id,
        "type": entity_type,
        "speed": {"type": "Property", "value": 7, "observedAt": "2025-01-01T00:00:00Z"},
        "@context": [CORE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);

    let retrieved = client.get_entity(&id, "", "application/ld+json", None).await.expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve entity: {:?}", retrieved.body);
    assert!(
        opens_with_context(text_of(&retrieved)),
        "an entity opens with @context: {}",
        text_of(&retrieved)
    );

    let queried = client
        .query_entities(&format!("type={entity_type}"), "application/ld+json", None)
        .await
        .expect("query entities request");
    assert_eq!(queried.status, StatusCode::OK, "query entities: {:?}", queried.body);
    assert_every_item_opens_with_context(&queried, 1, "entity");

    let temporal = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{}", urlencoding::encode(&id)))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(temporal.status, StatusCode::OK, "retrieve temporal entity: {:?}", temporal.body);
    assert!(
        opens_with_context(text_of(&temporal)),
        "a Temporal Evolution opens with @context: {}",
        text_of(&temporal)
    );

    client.delete_entity(&id).await.expect("delete entity request");
    scope.finish().await.expect("environment cleanup");
}

/// Clause 6.3.6: a notification delivered as `application/ld+json` opens with
/// `@context`, and so does every entity of its `data`.
#[tokio::test]
async fn regression_context_first_member_notification_body() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start");
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let entity_type = scope.entity_type("Vehicle");
    let subscription_id = scope.id("Subscription:context-first-notification");
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notification": {"endpoint": {"uri": endpoint, "accept": "application/ld+json"}},
        "@context": [CORE_CONTEXT]
    });
    let accepted = client.create_subscription(&subscription).await.expect("create subscription request");
    assert_eq!(accepted.status, StatusCode::CREATED, "create subscription: {:?}", accepted.body);
    sleep(Duration::from_millis(400)).await;

    // Table 5.2.12-1: absent `notificationTrigger` defaults to `attributeCreated`
    // and `attributeUpdated`, so the created entity carries an attribute to
    // create.
    let entity = json!({
        "id": scope.id("Vehicle:context-first-notified"),
        "type": entity_type,
        "speed": {"type": "Property", "value": 7},
        "@context": [CORE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);

    let notification = receiver.next_request(Duration::from_secs(10)).await.expect("a notification arrives");
    let text = from_utf8(&notification.body).expect("a JSON-LD notification is UTF-8");
    assert!(opens_with_context(text), "a notification opens with @context: {text}");
    assert!(text.contains(r#""data":[{"@context":"#), "every notified entity opens with @context: {text}");

    client.delete_subscription(&subscription_id).await.expect("delete subscription request");
    receiver.shutdown().await.expect("notification receiver shutdown failed");
    scope.finish().await.expect("environment cleanup");
}
