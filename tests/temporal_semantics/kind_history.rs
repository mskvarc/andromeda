//! Historical kind change.
//!
//! A history row carries the attribute family and value shape it was recorded
//! with. Those codes are on the row, not in a mutable catalogue keyed by
//! attribute name, so an attribute whose type changes later does not
//! retroactively change what was already recorded.

use crate::{
    attribute_iri,
    clean_database,
    client,
    create_entity,
    database,
    get_temporal_entity,
    rendered_instances,
    stored_history::stored_attribute_history,
    unique_id,
    update_entity_attributes,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

/// The stored code for the Property family.
const PROPERTY_KIND: i16 = 1;

/// The stored code for the Relationship family.
const RELATIONSHIP_KIND: i16 = 2;

/// The stored code for a numeric value.
const NUMBER_VALUE_KIND: i16 = 1;

/// The stored code for a string value.
const STRING_VALUE_KIND: i16 = 2;

/// The stored code for a URI value, which a Relationship target occupies.
const URI_VALUE_KIND: i16 = 7;

/// A later Relationship, and a later differently shaped Property, leave the
/// numeric Property already recorded at the same grain exactly as it was.
#[tokio::test]
#[serial]
async fn temporal_semantics_a_later_kind_change_does_not_reinterpret_recorded_history() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Sensor:kind_change");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Sensor",
            "signal": { "type": "Property", "value": 5, "observedAt": "2021-01-01T00:00:00Z" }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let recorded_first = stored_attribute_history(&pool, &entity_id, &attribute_iri("signal")).await;
    assert_eq!(recorded_first.len(), 1);
    let original = recorded_first[0].clone();
    assert_eq!(original.attribute_kind, PROPERTY_KIND);
    assert_eq!(original.value_kind, NUMBER_VALUE_KIND);
    assert_eq!(original.value_number.as_deref(), Some("5"));

    // The same grain becomes a Relationship, then a string Property.
    let updated = update_entity_attributes(
        &http,
        &entity_id,
        &json!({ "signal": { "type": "Relationship", "object": "urn:ngsi-ld:Thing:1", "observedAt": "2021-02-01T00:00:00Z" } }),
    )
    .await;
    assert_eq!(updated, StatusCode::NO_CONTENT);
    let updated = update_entity_attributes(
        &http,
        &entity_id,
        &json!({ "signal": { "type": "Property", "value": "amber", "observedAt": "2021-03-01T00:00:00Z" } }),
    )
    .await;
    assert_eq!(updated, StatusCode::NO_CONTENT);

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("signal")).await;
    assert_eq!(rows.len(), 3, "each change is its own recorded instance");
    assert_eq!(rows[0].stream_id, rows[2].stream_id, "all three sit at the same clause 4.5.5 grain");

    // The first row is byte for byte what it was, after two later kind changes.
    assert_eq!(rows[0].attribute_kind, original.attribute_kind, "the recorded family is not reinterpreted");
    assert_eq!(rows[0].value_kind, original.value_kind, "the recorded value shape is not reinterpreted");
    assert_eq!(rows[0].value_number, original.value_number, "the recorded value is unchanged");
    assert_eq!(rows[0].generated_instance_uuid, original.generated_instance_uuid);

    // Each later row carries its own codes rather than the catalogue's newest.
    assert_eq!(rows[1].attribute_kind, RELATIONSHIP_KIND);
    assert_eq!(rows[1].value_kind, URI_VALUE_KIND);
    assert_eq!(rows[1].value_uri.as_deref(), Some("urn:ngsi-ld:Thing:1"));
    assert_eq!(rows[2].attribute_kind, PROPERTY_KIND);
    assert_eq!(rows[2].value_kind, STRING_VALUE_KIND);
    assert_eq!(rows[2].value_string.as_deref(), Some("amber"));

    // Reconstruction reproduces the original instance identically.
    let (status, body) = get_temporal_entity(&http, &entity_id, "timerel=between&timeAt=2020-12-31T00:00:00Z&endTimeAt=2021-01-02T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK);
    let instances = rendered_instances(&body, "signal");
    assert_eq!(instances.len(), 1, "only the first observation lies in that window");
    assert_eq!(instances[0]["type"], json!("Property"), "the first instance is still a Property");
    assert_eq!(instances[0]["value"], json!(5), "the first instance still carries its number");
    assert!(instances[0].get("object").is_none(), "the later Relationship did not reshape it");
}
