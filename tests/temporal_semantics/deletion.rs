//! Tombstones and the three deletions the specification defines.
//!
//! Clauses 4.5.7 and 4.5.8 make attribute deletion an observation carrying the
//! null representation, clause 5.6.6 removes current Entity state while leaving
//! Temporal Evolution standing, clause 5.6.16 removes Temporal Evolution while
//! leaving current state standing, and clause 5.6.15 removes exactly one named
//! instance.

use crate::{
    attribute_iri,
    clean_database,
    client,
    create_entity,
    create_temporal_entity,
    database,
    delete_entity,
    delete_entity_attribute,
    delete_temporal_entity,
    delete_temporal_instance,
    get_entity,
    get_temporal_entity,
    rendered_instances,
    stored_history::{current_projection_rows, orphaned_temporal_rows, stored_attribute_history, stored_history, temporal_catalog_rows},
    unique_id,
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;

/// The reserved tombstone value-shape code.
const TOMBSTONE_VALUE_KIND: i16 = 0;

/// The stored code for the Property family.
const PROPERTY_KIND: i16 = 1;

/// The stored code for the Relationship family.
const RELATIONSHIP_KIND: i16 = 2;

/// The null representation clauses 4.5.7 and 4.5.8 require deletion to render.
const NULL_REPRESENTATION: &str = "urn:ngsi-ld:null";

/// Clauses 4.5.7 and 4.5.8: deleting a Property and a Relationship records the
/// reserved tombstone kind with no typed value, and rendering synthesises the
/// null representation in the member shape each family dictates.
#[tokio::test]
#[serial]
async fn temporal_semantics_tombstones_carry_no_typed_value_and_render_the_null_representation() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:tombstones");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 55 },
            "owner": { "type": "Relationship", "object": "urn:ngsi-ld:Person:1" }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);
    assert_eq!(current_projection_rows(&pool, &entity_id).await, 2, "both attributes are projected");

    assert_eq!(delete_entity_attribute(&http, &entity_id, "speed").await, StatusCode::NO_CONTENT);
    assert_eq!(delete_entity_attribute(&http, &entity_id, "owner").await, StatusCode::NO_CONTENT);

    // The deletion is an observation, not a mutation of the recorded one.
    for (term, attribute_kind) in [("speed", PROPERTY_KIND), ("owner", RELATIONSHIP_KIND)] {
        let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri(term)).await;
        assert_eq!(rows.len(), 2, "{term} kept its observation and gained a tombstone");
        let tombstone = rows[1].clone();
        assert_eq!(tombstone.value_kind, TOMBSTONE_VALUE_KIND, "{term} deletion uses the reserved kind");
        assert!(tombstone.deleted_at.is_some(), "{term} tombstone carries deletedAt");
        assert_eq!(tombstone.populated_value_columns, 0, "{term} tombstone stores no typed value");
        assert!(tombstone.value_uri.is_none(), "{term} tombstone stores no fake URI value");
        assert!(tombstone.value_string.is_none(), "{term} tombstone stores no null sentinel string");
        assert_eq!(
            tombstone.attribute_kind, attribute_kind,
            "{term} tombstone keeps the family whose member shape rendering needs"
        );
    }

    // The current projection row is gone; a deleted attribute is an absent row.
    assert_eq!(
        current_projection_rows(&pool, &entity_id).await,
        0,
        "a deleted attribute leaves no projection row carrying a sentinel"
    );

    // Rendering synthesises the null representation in each family's shape.
    let (status, body) = get_temporal_entity(&http, &entity_id, "timeproperty=deletedAt&timerel=after&timeAt=1970-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK);
    let speed = rendered_instances(&body, "speed");
    assert_eq!(speed.len(), 1);
    assert_eq!(speed[0]["type"], json!("Property"));
    assert_eq!(speed[0]["value"], json!(NULL_REPRESENTATION), "a Property deletion renders a null value");
    let owner = rendered_instances(&body, "owner");
    assert_eq!(owner.len(), 1);
    assert_eq!(owner[0]["type"], json!("Relationship"), "clause 4.5.8 keeps the Relationship family");
    assert_eq!(owner[0]["object"], json!(NULL_REPRESENTATION), "a Relationship deletion renders a null object");
}

/// Clause 5.6.6: Core Entity deletion removes current state and leaves the
/// Temporal Evolution queryable.
#[tokio::test]
#[serial]
async fn temporal_semantics_core_entity_deletion_leaves_temporal_state_standing() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:core_deletion");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 55, "observedAt": "2021-01-01T00:00:00Z" }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);
    let before = stored_history(&pool, &entity_id).await;
    assert_eq!(before.len(), 1);

    assert_eq!(delete_entity(&http, &entity_id).await, StatusCode::NO_CONTENT);

    let (status, _) = get_entity(&http, &entity_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "current Entity state is gone");
    assert_eq!(
        current_projection_rows(&pool, &entity_id).await,
        0,
        "the current projection went with the Entity through one cascade"
    );

    // Temporal state is independent and survives.
    let (entities, streams) = temporal_catalog_rows(&pool, &entity_id).await;
    assert_eq!(entities, 1, "the temporal Entity is not owned by current state");
    assert_eq!(streams, 1, "its streams stand too");
    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK, "the Temporal Evolution is still queryable");
    assert!(!rendered_instances(&body, "speed").is_empty(), "recorded history survives Core deletion");
}

/// Clause 5.6.16: Temporal Evolution deletion removes every temporal row and
/// leaves current Entity state unchanged.
#[tokio::test]
#[serial]
async fn temporal_semantics_temporal_evolution_deletion_leaves_current_state_unchanged() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:evolution_deletion");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 55, "observedAt": "2021-01-01T00:00:00Z" }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    assert_eq!(delete_temporal_entity(&http, &entity_id).await, StatusCode::NO_CONTENT);

    assert!(stored_history(&pool, &entity_id).await.is_empty(), "all history rows are gone");
    assert_eq!(
        temporal_catalog_rows(&pool, &entity_id).await,
        (0, 0),
        "the temporal Entity and its streams are gone"
    );
    assert_eq!(
        orphaned_temporal_rows(&pool).await,
        (0, 0, 0),
        "no history row, scope row or stream was left without its owner"
    );
    let (status, _) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Current state is a different authority and is untouched.
    let (status, entity) = get_entity(&http, &entity_id).await;
    assert_eq!(status, StatusCode::OK, "current Entity state survives clause 5.6.16");
    assert_eq!(entity["speed"]["value"], json!(55));
    assert_eq!(current_projection_rows(&pool, &entity_id).await, 1, "the current projection is unchanged");
}

/// Clause 5.6.15: instance deletion removes exactly the named instance,
/// physically, and leaves no soft-deleted row behind.
#[tokio::test]
#[serial]
async fn temporal_semantics_temporal_instance_deletion_removes_exactly_the_named_instance() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:instance_deletion");

    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": [
                { "type": "Property", "value": 1, "observedAt": "2021-01-01T00:00:00Z" },
                { "type": "Property", "value": 2, "observedAt": "2021-01-02T00:00:00Z" }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    let instances = rendered_instances(&body, "speed");
    assert_eq!(instances.len(), 2);
    let doomed = instances[0]["instanceId"].as_str().expect("a rendered instance identifier").to_owned();
    let surviving = instances[1]["instanceId"].as_str().expect("a rendered instance identifier").to_owned();

    assert_eq!(delete_temporal_instance(&http, &entity_id, "speed", &doomed).await, StatusCode::NO_CONTENT);

    // The removal is physical: one row fewer, and no tombstone stands in for it.
    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 1, "exactly the named instance was removed");
    assert_eq!(rows[0].value_number.as_deref(), Some("2"), "the other instance remains");
    assert!(
        rows.iter().all(|row| row.deleted_at.is_none() && row.value_kind != TOMBSTONE_VALUE_KIND),
        "clause 5.6.15 removes the row rather than soft-deleting it"
    );

    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    let remaining = rendered_instances(&body, "speed");
    assert_eq!(remaining.len(), 1, "retrieval shows no soft-deleted row");
    assert_eq!(remaining[0]["instanceId"].as_str(), Some(surviving.as_str()));
    assert!(
        !remaining
            .iter()
            .any(|instance| instance.get("instanceId").and_then(Value::as_str) == Some(doomed.as_str())),
        "the removed instance is unreachable"
    );
}
