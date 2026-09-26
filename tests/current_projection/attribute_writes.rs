//! Verification-matrix row "Attribute write consequences".
//!
//! Each attribute-level Core API write (clauses 5.6.2, 5.6.3, 5.6.4 and
//! 5.6.19) must leave the same attribute instance in the Entity, in its
//! Temporal Evolution and in the current-value projection a `q` filter reads.
//! Recording the request Fragment instead loses what the write kept: the
//! `unitCode` clause 5.5.8 EXAMPLE 1 leaves in place, or the value a
//! `noOverwrite` append refused to replace.

use crate::{
    projection_rows::{rows_for_attribute, unit_code_of},
    write_consequences::{
        core_context,
        create,
        current_attribute,
        instance_valued,
        instances_of_dataset,
        q_selects,
        temporal_attribute_names,
        temporal_instances,
        write,
    },
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use urlencoding::encode;

/// The `observedAt` of the stored instance in clause 5.5.8 EXAMPLE 1.
const FIRST_OBSERVED_AT: &str = "2022-03-14T01:59:26.535Z";

/// The `observedAt` the partial update of clause 5.5.8 EXAMPLE 1 supplies.
const SECOND_OBSERVED_AT: &str = "2022-03-14T13:00:00.5Z";

#[tokio::test]
async fn current_projection_partial_attribute_update_keeps_the_unit_code_in_history_projection_and_query() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:partial-unit-code");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({
            "id": id,
            "type": entity_type,
            "temperature": {"type": "Property", "value": 25, "unitCode": "CEL", "observedAt": FIRST_OBSERVED_AT},
            "@context": core_context()
        }),
    )
    .await;

    // Clause 5.5.8 EXAMPLE 1: the Fragment restates value and observedAt only.
    write(
        client,
        Method::PATCH,
        &format!("entities/{}/attrs/temperature", encode(&id)),
        &json!({"type": "Property", "value": 100, "observedAt": SECOND_OBSERVED_AT, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;

    let current = current_attribute(client, &id, "temperature").await;
    assert_eq!(current["value"], json!(100));
    assert_eq!(current["unitCode"], json!("CEL"), "clause 5.5.8 EXAMPLE 1 leaves unitCode untouched");

    let history = temporal_instances(client, &id, "temperature").await;
    assert_eq!(history.len(), 2, "the create and the partial update each add one instance: {history:?}");
    let updated = instance_valued(&history, &json!(100));
    assert_eq!(
        updated["unitCode"],
        json!("CEL"),
        "the recorded instance is the resulting one, carrying unitCode"
    );
    assert!(
        updated.get("observedAt").is_some(),
        "the recorded instance carries the observedAt the update supplied"
    );

    let rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    assert_eq!(rows.len(), 1, "one projected grain: {rows:?}");
    assert_eq!(rows[0].value_text.as_deref(), Some("100"));
    assert_eq!(
        unit_code_of(scope.database_pool(), &id, "temperature", None).await.as_deref(),
        Some("CEL"),
        "the projection keeps the unitCode the Entity kept"
    );

    assert!(q_selects(client, &entity_type, &id, "temperature==100").await, "q must see the updated value");
    assert!(
        !q_selects(client, &entity_type, &id, "temperature==25").await,
        "q must no longer see the old value"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_partial_attribute_update_keeps_a_sub_property_inside_its_attribute() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:partial-sub-property");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25}, "@context": core_context()}),
    )
    .await;

    write(
        client,
        Method::PATCH,
        &format!("entities/{}/attrs/temperature", encode(&id)),
        &json!({
            "type": "Property",
            "value": 26,
            "accuracy": {"type": "Property", "value": 0.5},
            "@context": core_context()
        }),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert!(
        rows_for_attribute(scope.database_pool(), &id, "accuracy").await.is_empty(),
        "a sub-Property is part of its attribute's instance, never an Entity attribute of its own"
    );
    let history = temporal_instances(client, &id, "temperature").await;
    assert_eq!(instance_valued(&history, &json!(26))["accuracy"]["value"], json!(0.5));
    assert_eq!(
        temporal_attribute_names(client, &id).await,
        vec!["temperature".to_owned()],
        "no Temporal Evolution for the sub-Property"
    );
    assert!(q_selects(client, &entity_type, &id, "temperature==26").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_partial_attribute_update_of_one_dataset_leaves_the_other_instance_alone() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:partial-dataset");
    let entity_type = scope.entity_type("Vehicle");
    create(
        client,
        &json!({
            "id": id,
            "type": entity_type,
            "speed": [
                {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:gps"},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:odometer"}
            ],
            "@context": core_context()
        }),
    )
    .await;

    // Clause 5.6.4.4 addresses the instance "with the specified datasetId".
    write(
        client,
        Method::PATCH,
        &format!("entities/{}/attrs/speed", encode(&id)),
        &json!({"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:odometer", "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;

    let history = temporal_instances(client, &id, "speed").await;
    assert_eq!(
        instances_of_dataset(&history, "urn:ngsi-ld:Dataset:gps").len(),
        1,
        "the untouched instance gains no history: {history:?}"
    );
    assert_eq!(
        instances_of_dataset(&history, "urn:ngsi-ld:Dataset:odometer").len(),
        2,
        "the addressed instance gains one: {history:?}"
    );

    let rows = rows_for_attribute(scope.database_pool(), &id, "speed").await;
    let projected: Vec<(Option<&str>, Option<&str>)> = rows.iter().map(|row| (row.dataset_id.as_deref(), row.value_text.as_deref())).collect();
    assert_eq!(
        projected,
        vec![(Some("urn:ngsi-ld:Dataset:gps"), Some("1")), (Some("urn:ngsi-ld:Dataset:odometer"), Some("3"))]
    );
    assert!(q_selects(client, &entity_type, &id, "speed==3").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_replace_attribute_records_the_replacement_in_history_projection_and_query() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:replace-attribute");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25, "unitCode": "CEL"}, "@context": core_context()}),
    )
    .await;

    // Clause 5.6.19.4 replaces the instance whole, so its unitCode goes.
    write(
        client,
        Method::PUT,
        &format!("entities/{}/attrs/temperature", encode(&id)),
        &json!({"type": "Property", "value": 30, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert!(current_attribute(client, &id, "temperature").await.get("unitCode").is_none());
    let history = temporal_instances(client, &id, "temperature").await;
    assert_eq!(history.len(), 2, "{history:?}");
    assert!(
        instance_valued(&history, &json!(30)).get("unitCode").is_none(),
        "the replacement carries no unitCode"
    );

    let rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].value_text.as_deref(), Some("30"));
    assert_eq!(unit_code_of(scope.database_pool(), &id, "temperature", None).await, None);
    assert!(q_selects(client, &entity_type, &id, "temperature==30").await);
    assert!(!q_selects(client, &entity_type, &id, "temperature==25").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_update_attributes_overwrites_the_whole_instance_everywhere() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:update-attributes");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25, "unitCode": "CEL"}, "@context": core_context()}),
    )
    .await;

    // Clause 5.5.8 EXAMPLE 2: Update Attributes replaces the whole attribute.
    write(
        client,
        Method::PATCH,
        &format!("entities/{}/attrs", encode(&id)),
        &json!({"temperature": {"type": "Property", "value": 100}, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;

    assert!(current_attribute(client, &id, "temperature").await.get("unitCode").is_none());
    let history = temporal_instances(client, &id, "temperature").await;
    assert!(instance_valued(&history, &json!(100)).get("unitCode").is_none());
    assert_eq!(unit_code_of(scope.database_pool(), &id, "temperature", None).await, None);
    assert!(q_selects(client, &entity_type, &id, "temperature==100").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_append_without_overwrite_leaves_the_existing_value_everywhere() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Sensor:append-no-overwrite");
    let entity_type = scope.entity_type("Sensor");
    create(
        client,
        &json!({"id": id, "type": entity_type, "temperature": {"type": "Property", "value": 25}, "@context": core_context()}),
    )
    .await;

    // Clause 5.6.3.4: an existing attribute "shall be left untouched".
    write(
        client,
        Method::POST,
        &format!("entities/{}/attrs?options=noOverwrite", encode(&id)),
        &json!({
            "temperature": {"type": "Property", "value": 99},
            "name": {"type": "Property", "value": "lobby"},
            "@context": core_context()
        }),
        StatusCode::MULTI_STATUS,
    )
    .await;

    assert_eq!(current_attribute(client, &id, "temperature").await["value"], json!(25));
    assert_eq!(
        temporal_instances(client, &id, "temperature").await.len(),
        1,
        "the skipped value is not recorded"
    );
    assert_eq!(
        rows_for_attribute(scope.database_pool(), &id, "temperature").await[0].value_text.as_deref(),
        Some("25")
    );
    assert!(
        q_selects(client, &entity_type, &id, "temperature==25").await,
        "q still sees the value the Entity holds"
    );
    assert!(
        !q_selects(client, &entity_type, &id, "temperature==99").await,
        "q must not see the value that was refused"
    );

    assert_eq!(temporal_instances(client, &id, "name").await.len(), 1, "the appended attribute is recorded");
    assert_eq!(
        rows_for_attribute(scope.database_pool(), &id, "name").await[0].value_text.as_deref(),
        Some("lobby")
    );

    scope.finish().await.expect("cleanup failed");
}
