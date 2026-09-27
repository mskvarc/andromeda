//! Entity write consequences: whole-entity and batch writes leave one
//! consistent attribute instance everywhere it is stored.
//!
//! The whole-entity and batch Core API writes (clauses 5.6.8, 5.6.9, 5.6.17,
//! 5.6.18 and 5.6.20) must leave the same attribute instance in the Entity, in
//! its Temporal Evolution and in the current-value projection a `q` filter
//! reads. A merge reaches into the stored instance (clause 5.5.12), so a
//! record of its Fragment would lose both the `unitCode` of EXAMPLE 1 and the
//! value members EXAMPLE 2 keeps.

use crate::{
    projection_rows::{rows_for_attribute, unit_code_of},
    write_consequences::{core_context, create, current_attribute, instance_valued, q_selects, temporal_instances, write},
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use urlencoding::encode;

/// The stored `address` of clause 5.5.12 EXAMPLE 2.
fn berlin_address() -> Value {
    json!({"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"})
}

/// The `address` clause 5.5.12 EXAMPLE 2 merges it into.
fn merged_address() -> Value {
    json!({"street": "Pariser Platz", "city": "Berlin"})
}

#[tokio::test]
async fn current_projection_merge_entity_records_the_deep_merged_value() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:merge-address");
    let entity_type = scope.entity_type("Building");
    create(
        client,
        &json!({"id": id, "type": entity_type, "address": {"type": "Property", "value": berlin_address()}, "@context": core_context()}),
    )
    .await;

    // Clause 5.5.12 EXAMPLE 2: street is replaced and country removed.
    write(
        client,
        Method::PATCH,
        &format!("entities/{}", encode(&id)),
        &json!({
            "address": {"type": "Property", "value": {"street": "Pariser Platz", "country": "urn:ngsi-ld:null"}},
            "@context": core_context()
        }),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert_eq!(current_attribute(client, &id, "address").await["value"], merged_address());
    let history = temporal_instances(client, &id, "address").await;
    assert_eq!(history.len(), 2, "{history:?}");
    instance_valued(&history, &merged_address());

    let rows = rows_for_attribute(scope.database_pool(), &id, "address").await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    // The projection stores the object in its expanded form, one member per
    // expanded key.
    let projected: Value = serde_json::from_str(rows[0].value_text.as_deref().expect("a projected object")).expect("projected JSON");
    let mut members: Vec<(&str, &Value)> = projected
        .as_object()
        .expect("a projected object")
        .iter()
        .map(|(member, value)| (member.rsplit('/').next().unwrap_or(member), &value[0]["@value"]))
        .collect();
    members.sort_by_key(|(member, _)| *member);
    assert_eq!(
        members,
        vec![("city", &json!("Berlin")), ("street", &json!("Pariser Platz"))],
        "the projection holds the merged object, not the Fragment's part of it"
    );
    assert!(
        q_selects(client, &entity_type, &id, "address[city]==\"Berlin\"").await,
        "q still sees the member the merge kept"
    );
    assert!(q_selects(client, &entity_type, &id, "address[street]==\"Pariser Platz\"").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_merge_entity_keeps_the_unit_code_in_history_and_projection() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:merge-unit-code");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25, "unitCode": "CEL"}, "@context": core_context()}),
    )
    .await;

    // Clause 5.5.12 EXAMPLE 1 leaves the unitCode the Fragment omits.
    write(
        client,
        Method::PATCH,
        &format!("entities/{}", encode(&id)),
        &json!({"temperature": {"type": "Property", "value": 100}, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert_eq!(current_attribute(client, &id, "temperature").await["unitCode"], json!("CEL"));
    assert_eq!(
        instance_valued(&temporal_instances(client, &id, "temperature").await, &json!(100))["unitCode"],
        json!("CEL")
    );
    assert_eq!(unit_code_of(scope.database_pool(), &id, "temperature", None).await.as_deref(), Some("CEL"));
    assert!(q_selects(client, &entity_type, &id, "temperature==100").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_replace_entity_records_every_attribute_of_the_replacement() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:replace-entity");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25, "unitCode": "CEL"}, "@context": core_context()}),
    )
    .await;

    write(
        client,
        Method::PUT,
        &format!("entities/{}", encode(&id)),
        &json!({
            "id": id,
            "type": entity_type,
            "temperature": {"type": "Property", "value": 40},
            "name": {"type": "Property", "value": "lobby"},
            "@context": core_context()
        }),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert!(
        instance_valued(&temporal_instances(client, &id, "temperature").await, &json!(40))
            .get("unitCode")
            .is_none()
    );
    assert_eq!(temporal_instances(client, &id, "name").await.len(), 1);
    assert_eq!(unit_code_of(scope.database_pool(), &id, "temperature", None).await, None);
    assert!(q_selects(client, &entity_type, &id, "temperature==40").await);
    assert!(q_selects(client, &entity_type, &id, "name==\"lobby\"").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_batch_merge_keeps_the_unit_code_in_history_and_projection() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:batch-merge");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25, "unitCode": "CEL"}, "@context": core_context()}),
    )
    .await;

    // Clause 5.6.20.4 runs the merge of clause 5.6.17 per Entity.
    write(
        client,
        Method::POST,
        "entityOperations/merge",
        &json!([{"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 100}, "@context": core_context()}]),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert_eq!(
        instance_valued(&temporal_instances(client, &id, "temperature").await, &json!(100))["unitCode"],
        json!("CEL")
    );
    assert_eq!(unit_code_of(scope.database_pool(), &id, "temperature", None).await.as_deref(), Some("CEL"));
    assert!(q_selects(client, &entity_type, &id, "temperature==100").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_batch_update_without_overwrite_leaves_the_existing_value_everywhere() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:batch-no-overwrite");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25}, "@context": core_context()}),
    )
    .await;

    // Clause 5.6.9.4 with noOverwrite is an Append Attributes that leaves an
    // existing attribute untouched; appending nothing reports the Entity as an
    // error (clause 5.6.9.5).
    write(
        client,
        Method::POST,
        "entityOperations/update?options=noOverwrite",
        &json!([{"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 99}, "@context": core_context()}]),
        StatusCode::MULTI_STATUS,
    )
    .await;

    assert_eq!(current_attribute(client, &id, "temperature").await["value"], json!(25));
    assert_eq!(
        temporal_instances(client, &id, "temperature").await.len(),
        1,
        "the refused value is not recorded"
    );
    assert_eq!(
        rows_for_attribute(scope.database_pool(), &id, "temperature").await[0].value_text.as_deref(),
        Some("25")
    );
    assert!(
        !q_selects(client, &entity_type, &id, "temperature==99").await,
        "q must not see the refused value"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_batch_upsert_update_overwrites_the_whole_instance_everywhere() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:batch-upsert-update");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25, "unitCode": "CEL"}, "@context": core_context()}),
    )
    .await;

    // Clause 5.6.8.4 update mode runs Update Attributes (clause 5.6.2).
    write(
        client,
        Method::POST,
        "entityOperations/upsert?options=update",
        &json!([{"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 100}, "@context": core_context()}]),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert!(
        instance_valued(&temporal_instances(client, &id, "temperature").await, &json!(100))
            .get("unitCode")
            .is_none()
    );
    assert_eq!(unit_code_of(scope.database_pool(), &id, "temperature", None).await, None);
    assert!(q_selects(client, &entity_type, &id, "temperature==100").await);

    scope.finish().await.expect("cleanup failed");
}
