//! Attribute-instance `createdAt`/`modifiedAt` are broker-assigned, per
//! instance, and only shown when the request asks for them.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.8 defines `createdAt` and `modifiedAt` for
//! "the Entity, Property or Relationship", so every attribute instance carries
//! its own pair, and Table 5.2.14-1 includes them in a representation only when
//! `sysAttrs` is requested. Clause 4.5.5 makes the instance identity
//! `(attribute name, datasetId)`, so the pair is per instance. Andromeda stamps
//! them into the stored payload on a write -- a created instance takes the write
//! instant for both, an updated one keeps its `createdAt` and moves its
//! `modifiedAt` -- and strips them from any read that did not ask for
//! `sysAttrs`. Case `019_19_04` orders a query by `name.createdAt`, which
//! clause 4.23.3 EXAMPLE 5 spells as a system sub-attribute of an attribute and
//! which only resolves once the stamp is in the stored payload.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

/// A pause between two writes, so a later write's stamp is unambiguously after
/// an earlier one regardless of clock resolution.
const STAMP_GAP: Duration = Duration::from_millis(1100);

/// A Vehicle with one `name` Property.
fn vehicle(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "name": {"type": "Property", "value": name},
        "@context": [CORE_CONTEXT]
    })
}

/// Create an entity and assert it was taken.
async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create: {:?}", response.body);
}

/// Read one entity in normalized form, with or without the system attributes.
async fn read(client: &TestClient, id: &str, sys_attrs: bool) -> Value {
    let query = if sys_attrs { "options=sysAttrs" } else { "" };
    let response = client
        .get_entity(id, query, "application/json", Some(CORE_CONTEXT.as_str()))
        .await
        .expect("retrieval request");
    assert_eq!(response.status, StatusCode::OK, "read: {:?}", response.body);
    response.json_value().expect("a retrieval answers JSON")
}

/// The single-instance attribute node named `attr`.
fn attribute<'a>(entity: &'a Value, attr: &str) -> &'a Value {
    &entity[attr]
}

/// The attribute instance carrying `dataset` among an attribute's instances.
fn dataset_instance<'a>(entity: &'a Value, attr: &str, dataset: &str) -> &'a Value {
    entity[attr]
        .as_array()
        .expect("a multi-instance attribute is an array")
        .iter()
        .find(|instance| instance["datasetId"] == json!(dataset))
        .expect("the requested dataset instance is present")
}

/// A system time member of an attribute node, as text.
fn instant(node: &Value, member: &str) -> String {
    node[member].as_str().expect("a system time is a string").to_string()
}

/// Clause 4.8 with Table 5.2.14-1: a freshly created attribute instance carries
/// `createdAt` and `modifiedAt`, equal to the create instant, only under
/// `sysAttrs`.
#[tokio::test]
async fn regression_attribute_system_timestamps_a_create_stamps_equal_created_and_modified() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:sysattr-create");

    create(client, &vehicle(&id, "first")).await;

    let entity = read(client, &id, true).await;
    let name = attribute(&entity, "name");
    let created = instant(name, "createdAt");
    let modified = instant(name, "modifiedAt");
    assert_eq!(created, modified, "a created instance's createdAt and modifiedAt are the write instant: {name}");

    client.delete_entity(&id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.14-1: a read that did not ask for `sysAttrs` carries neither
/// attribute-level system time, even though the stored payload holds them.
#[tokio::test]
async fn regression_attribute_system_timestamps_a_plain_read_hides_them() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:sysattr-hidden");

    create(client, &vehicle(&id, "first")).await;

    let entity = read(client, &id, false).await;
    let name = attribute(&entity, "name");
    assert!(name.get("createdAt").is_none(), "a plain read hides attribute createdAt: {name}");
    assert!(name.get("modifiedAt").is_none(), "a plain read hides attribute modifiedAt: {name}");

    let with_sys = read(client, &id, true).await;
    assert!(
        with_sys["name"].get("createdAt").is_some(),
        "a sysAttrs read shows attribute createdAt: {with_sys}"
    );

    client.delete_entity(&id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.8: updating an instance keeps its `createdAt` and moves its
/// `modifiedAt` to the new write instant, so `createdAt` survives the
/// read-modify-write.
#[tokio::test]
async fn regression_attribute_system_timestamps_an_update_keeps_created_and_moves_modified() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:sysattr-update");

    create(client, &vehicle(&id, "first")).await;
    let before = read(client, &id, true).await;
    let created_before = instant(attribute(&before, "name"), "createdAt");

    sleep(STAMP_GAP).await;
    let fragment = json!({"type": "Property", "value": "second", "@context": [CORE_CONTEXT]});
    let response = client.partial_update_attr(&id, "name", &fragment).await.expect("partial update request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "partial update: {:?}", response.body);

    let after = read(client, &id, true).await;
    let name = attribute(&after, "name");
    assert_eq!(instant(name, "createdAt"), created_before, "createdAt survives the update: {name}");
    assert_ne!(instant(name, "modifiedAt"), created_before, "modifiedAt moved to the update instant: {name}");

    client.delete_entity(&id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.5: the system times are per instance, so updating one `datasetId`
/// leaves another instance's times untouched.
#[tokio::test]
async fn regression_attribute_system_timestamps_each_dataset_instance_keeps_its_own() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:sysattr-datasets");

    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "name": [
            {"type": "Property", "value": "a", "datasetId": "urn:ngsi-ld:Dataset:A"},
            {"type": "Property", "value": "b", "datasetId": "urn:ngsi-ld:Dataset:B"}
        ],
        "@context": [CORE_CONTEXT]
    });
    create(client, &entity).await;

    sleep(STAMP_GAP).await;
    let fragment = json!({"type": "Property", "value": "a2", "datasetId": "urn:ngsi-ld:Dataset:A", "@context": [CORE_CONTEXT]});
    let response = client.partial_update_attr(&id, "name", &fragment).await.expect("partial update request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "partial update: {:?}", response.body);

    let after = read(client, &id, true).await;
    let instance_a = dataset_instance(&after, "name", "urn:ngsi-ld:Dataset:A");
    let instance_b = dataset_instance(&after, "name", "urn:ngsi-ld:Dataset:B");
    assert_ne!(
        instant(instance_a, "modifiedAt"),
        instant(instance_a, "createdAt"),
        "the updated dataset moved its modifiedAt: {instance_a}"
    );
    assert_eq!(
        instant(instance_b, "modifiedAt"),
        instant(instance_b, "createdAt"),
        "the untouched dataset kept its modifiedAt: {instance_b}"
    );

    client.delete_entity(&id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Case `019_19_04`, clause 4.23.3 EXAMPLE 5: a query ordered by
/// `name.createdAt` sorts entities by the stored stamp of that attribute.
#[tokio::test]
async fn regression_attribute_system_timestamps_order_by_created_at_uses_the_stamp() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let earlier = scope.id("Vehicle:sysattr-order-earlier");
    let later = scope.id("Vehicle:sysattr-order-later");

    create(client, &vehicle(&earlier, "e")).await;
    sleep(STAMP_GAP).await;
    create(client, &vehicle(&later, "l")).await;

    // The two ids constrain the result to this test's own entities, so the
    // ordering assertion does not depend on what else the suite created.
    let query = format!("type=Vehicle&id={earlier},{later}&orderBy=name.createdAt");
    let response = client
        .query_entities(&query, "application/json", Some(CORE_CONTEXT.as_str()))
        .await
        .expect("query request");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    let body = response.json_value().expect("a query answers JSON");
    let order: Vec<String> = body
        .as_array()
        .expect("a query answers an array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(String::from))
        .collect();

    let earlier_at = order.iter().position(|id| id == &earlier);
    let later_at = order.iter().position(|id| id == &later);
    assert!(earlier_at.is_some() && later_at.is_some(), "both entities are in the result: {order:?}");
    assert!(
        earlier_at < later_at,
        "ascending order by name.createdAt puts the earlier entity first: {order:?}"
    );

    client.delete_entity(&earlier).await.expect("delete entity");
    client.delete_entity(&later).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}
