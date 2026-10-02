//! What a notified entity is written as for each kind of change (clause
//! 5.8.6), every part of it by the typed walk.

use crate::processor::payload::notified_entity::{DeletedRendering, NotifiedEntity, rendered_entity};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_render::{
    entity_response::InlineJoin,
    notification_entity::{StampedInstants, TargetSpelling},
    notified_changes::HeldAttributes,
    structural_projection::StructuralProjection,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::{TimeZone, Utc};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::Arc;

/// An entity read from its compact document.
fn entity(document: &Value) -> Entity {
    deserialize_exact(document).expect("the entity decodes")
}

/// The projection that narrows nothing.
fn whole() -> Projection {
    Projection {
        pick: Vec::new(),
        omit: Vec::new(),
    }
}

/// A notified entity whose state is all the notification reports.
fn current(entity: Entity) -> NotifiedEntity {
    NotifiedEntity {
        entity: Arc::new(entity),
        stamped: StampedInstants::default(),
        held: HeldAttributes::Current,
        deleted: None,
        previous: None,
    }
}

/// A Vehicle with a speed and a Relationship.
fn vehicle() -> Entity {
    entity(&json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 80},
        "parkedIn": {"type": "Relationship", "object": "urn:ngsi-ld:Parking:a%2Fb"}
    }))
}

/// A notification that reports nothing beyond the entity's state writes the
/// entity as a query response does, its targets decoded.
#[test]
fn a_current_entity_is_written_as_it_stands() {
    let projection = whole();
    let rendered = current(vehicle())
        .render(
            ResponseFormat::Normalized,
            TargetSpelling::Decoded,
            None,
            StructuralProjection::from(&projection),
        )
        .expect("the walk writes it");
    assert_eq!(rendered["speed"], json!({"type": "Property", "value": 80}));
    assert_eq!(rendered["parkedIn"]["object"], json!("urn:ngsi-ld:Parking:a/b"));
}

/// An entity deletion under `showChanges` writes every Attribute deleted,
/// with its previous value, and the deletion instant on the entity.
#[test]
fn an_entity_deletion_reports_every_attribute_deleted_with_its_previous_value() {
    let previous = Arc::new(vehicle());
    let deleted_at = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant");
    let notified = NotifiedEntity {
        entity: Arc::clone(&previous),
        stamped: StampedInstants {
            deleted_at: Some(deleted_at),
            ..StampedInstants::default()
        },
        held: HeldAttributes::Deleted,
        deleted: None,
        previous: Some(previous),
    };
    let projection = whole();
    let rendered = notified
        .render(
            ResponseFormat::Normalized,
            TargetSpelling::Decoded,
            None,
            StructuralProjection::from(&projection),
        )
        .expect("the walk writes it");
    assert_eq!(rendered["deletedAt"], json!("2024-06-15T12:30:00Z"));
    assert_eq!(rendered["speed"], json!({"type": "Property", "value": "urn:ngsi-ld:null", "previousValue": 80}));
    assert_eq!(
        rendered["parkedIn"],
        json!({"type": "Relationship", "object": "urn:ngsi-ld:null", "previousObject": "urn:ngsi-ld:Parking:a/b"})
    );
}

/// The instances a change deleted are written beside the ones the entity
/// still holds.
#[test]
fn deleted_instances_join_the_held_ones() {
    let mut notified = current(vehicle());
    notified.deleted = Some(DeletedRendering {
        instances: Arc::new(entity(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 70, "datasetId": "urn:ngsi-ld:Dataset:old"}
        }))),
        deleted_at: None,
    });
    let projection = whole();
    let rendered = notified
        .render(
            ResponseFormat::Simplified,
            TargetSpelling::Decoded,
            None,
            StructuralProjection::from(&projection),
        )
        .expect("the walk writes it");
    assert_eq!(
        rendered["speed"],
        json!({"dataset": {"@none": 80, "urn:ngsi-ld:Dataset:old": "urn:ngsi-ld:null"}})
    );
}

/// Clause 4.5.23.2: an inline join nests the Linked Entity under the
/// Relationship naming it, in the notification's format.
#[test]
fn an_inline_join_nests_the_linked_entity() {
    let linked: IndexMap<_, _> = [(
        "urn:ngsi-ld:Parking:a%2Fb".parse().expect("an identifier"),
        entity(&json!({"id": "urn:ngsi-ld:Parking:a%2Fb", "type": "Parking", "free": {"type": "Property", "value": 3}})),
    )]
    .into_iter()
    .collect();
    let projection = whole();
    let rendered = current(vehicle())
        .render(
            ResponseFormat::Simplified,
            TargetSpelling::Decoded,
            Some(InlineJoin {
                linked: &linked,
                projection: None,
            }),
            StructuralProjection::from(&projection),
        )
        .expect("the walk writes it");
    assert_eq!(rendered["parkedIn"], json!({"id": "urn:ngsi-ld:Parking:a%2Fb", "type": "Parking", "free": 3}));
}

/// A Linked Entity a source answered with already nested is written in the
/// notification's format, and an entity typed like a Relationship family is
/// an entity like any other.
#[test]
fn a_carried_linked_entity_and_a_family_named_type_are_written_by_the_walk() {
    let typed_like_a_family = entity(&json!({"id": "urn:ngsi-ld:Link:1", "type": "Relationship"}));
    let rendered = rendered_entity(
        &typed_like_a_family,
        &StampedInstants::default(),
        ResponseFormat::Normalized,
        TargetSpelling::Decoded,
    )
    .expect("the walk writes it");
    assert_eq!(rendered, json!({"id": "urn:ngsi-ld:Link:1", "type": "Relationship"}));

    let carrying = entity(&json!({
        "id": "urn:ngsi-ld:Link:2", "type": "A",
        "rel": {"type": "Relationship", "object": "urn:ngsi-ld:B:1", "entity": {"id": "urn:ngsi-ld:B:1", "type": "B", "name": {"type": "Property", "value": "b"}}}
    }));
    let simplified = rendered_entity(&carrying, &StampedInstants::default(), ResponseFormat::Simplified, TargetSpelling::Decoded).expect("the walk writes it");
    assert_eq!(
        simplified,
        json!({"id": "urn:ngsi-ld:Link:2", "type": "A", "rel": {"id": "urn:ngsi-ld:B:1", "type": "B", "name": "b"}})
    );
}

/// A periodic notification spells its targets as the store holds them.
#[test]
fn targets_as_stored_are_left_encoded() {
    let rendered = rendered_entity(&vehicle(), &StampedInstants::default(), ResponseFormat::Normalized, TargetSpelling::AsStored).expect("the walk writes it");
    assert_eq!(rendered["parkedIn"]["object"], json!("urn:ngsi-ld:Parking:a%2Fb"));
}
