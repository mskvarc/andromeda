//! What a notification reports of a change beside the entity's state (clause
//! 5.8.6), as the typed walk writes it in every representation.

use crate::{
    notification_entity::{NotificationEntity, StampedInstants},
    notified_changes::{DeletedInstances, HeldAttributes, NotifiedChanges},
    structural_projection::StructuralProjection,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use chrono::{TimeZone, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// An entity read from its compact document.
fn entity(document: Value) -> Entity {
    serde_json::from_value(document).expect("an entity")
}

/// `notified` in `format` with `changes` reported beside it.
fn rendered(notified: &Entity, changes: NotifiedChanges<'_>, format: ResponseFormat) -> Value {
    let stamped = StampedInstants::default();
    let written = NotificationEntity::new(notified, &stamped, format).with_changes(changes);
    serde_json::to_value(written).expect("the walk writes it")
}

/// Every Attribute an entity held, one instance per family and one of two.
fn every_family() -> Entity {
    entity(json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "scope": "/Madrid",
        "speed": {"type": "Property", "value": 1, "unitCode": "KMH"},
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
        "parkedIn": {"type": "Relationship", "object": "urn:ngsi-ld:Parking:1"},
        "name": {"type": "LanguageProperty", "languageMap": {"en": "bus"}},
        "labels": [
            {"type": "LanguageProperty", "languageMap": {"en": "a"}},
            {"type": "LanguageProperty", "languageMap": {"en": "b"}, "datasetId": "urn:ngsi-ld:Dataset:b"}
        ],
        "category": {"type": "VocabProperty", "vocab": "commercial"},
        "config": {"type": "JsonProperty", "json": {"id": "1"}},
        "readings": {"type": "ListProperty", "valueList": [1, 2]},
        "rooms": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}]}
    }))
}

/// An entity deletion under `showChanges` reports every Attribute deleted.
fn entity_deletion(previous: Option<&Entity>) -> NotifiedChanges<'_> {
    NotifiedChanges {
        held: HeldAttributes::Deleted,
        deleted: None,
        previous,
    }
}

/// Clauses 4.5.0, 4.5.18, 4.5.21.2 and 4.5.22.2: every family takes the Null
/// it is deleted by in its own value member, every other member of the
/// instance stays, and the entity's own members are left alone. A
/// `LanguageProperty`, single or an instance of a multi-attribute, takes a
/// language map holding the Null (clause 4.5.18.2).
#[test]
fn an_entity_deletion_nullifies_every_attribute_in_its_own_value_member() {
    let notified = every_family();
    let document = rendered(&notified, entity_deletion(None), ResponseFormat::Normalized);
    assert_eq!(document["speed"], json!({"type": "Property", "value": "urn:ngsi-ld:null", "unitCode": "KMH"}));
    assert_eq!(document["location"]["value"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["parkedIn"]["object"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["name"]["languageMap"], json!({"@none": "urn:ngsi-ld:null"}));
    assert_eq!(document["labels"][0]["languageMap"], json!({"@none": "urn:ngsi-ld:null"}));
    assert_eq!(document["labels"][1]["languageMap"], json!({"@none": "urn:ngsi-ld:null"}));
    assert_eq!(document["category"]["vocab"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["config"]["json"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["readings"], json!({"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"]}));
    assert_eq!(document["rooms"], json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]}));
    assert_eq!(document["scope"], json!("/Madrid"));
    assert_eq!(document["type"], json!("Vehicle"));
}

/// Clause 5.8.6 with `showChanges`: each deleted instance carries the value
/// it held as its family's previous member, the multi-attribute's instances
/// each the one of its own `datasetId`.
#[test]
fn an_entity_deletion_reports_the_previous_value_of_every_instance() {
    let notified = every_family();
    let document = rendered(&notified, entity_deletion(Some(&notified)), ResponseFormat::Normalized);
    assert_eq!(document["speed"]["previousValue"], json!(1));
    assert_eq!(document["parkedIn"]["previousObject"], json!("urn:ngsi-ld:Parking:1"));
    assert_eq!(document["labels"][1]["previousLanguageMap"], json!({"en": "b"}));
    assert_eq!(document["readings"]["previousValueList"], json!([1, 2]));
    assert_eq!(document["rooms"]["previousObjectList"], json!([{"object": "urn:ngsi-ld:Room:1"}]));
    assert!(document["rooms"].get("previousObject").is_none(), "clause 4.5.22.2 prohibits previousObject");
}

/// Clause 4.5.4: a deleted instance's simplified value is the Null, kept in
/// the one member the language, vocabulary and JSON families keep.
#[test]
fn an_entity_deletion_simplifies_to_the_nulls() {
    let notified = every_family();
    let document = rendered(&notified, entity_deletion(Some(&notified)), ResponseFormat::Simplified);
    assert_eq!(document["speed"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["location"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["parkedIn"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["name"], json!({"languageMap": {"@none": "urn:ngsi-ld:null"}}));
    assert_eq!(
        document["labels"],
        json!({"dataset": {"@none": {"languageMap": {"@none": "urn:ngsi-ld:null"}}, "urn:ngsi-ld:Dataset:b": {"languageMap": {"@none": "urn:ngsi-ld:null"}}}})
    );
    assert_eq!(document["category"], json!({"vocab": "urn:ngsi-ld:null"}));
    assert_eq!(document["config"], json!({"json": "urn:ngsi-ld:null"}));
    assert_eq!(document["readings"], json!(["urn:ngsi-ld:null"]));
    assert_eq!(document["rooms"], json!(["urn:ngsi-ld:null"]));
}

/// Clause 4.5.2.3: a deleted Property that carries nothing else is the bare
/// Null in the concise representation; one carrying more keeps its object.
#[test]
fn a_concise_deletion_collapses_only_a_bare_property() {
    let notified = every_family();
    let document = rendered(&notified, entity_deletion(None), ResponseFormat::Concise);
    assert_eq!(document["location"], json!("urn:ngsi-ld:null"));
    assert_eq!(document["speed"], json!({"value": "urn:ngsi-ld:null", "unitCode": "KMH"}));
    assert_eq!(document["parkedIn"], json!({"object": "urn:ngsi-ld:null"}));
    assert_eq!(document["name"], json!({"languageMap": {"@none": "urn:ngsi-ld:null"}}));
}

/// Clause 5.8.6 with clause 5.5.11.0: one change that deleted two instances of
/// one attribute and the whole of another reports all three, nullified and
/// stamped with `deletedAt`, after the instances still held.
#[test]
fn every_instance_one_change_deleted_is_reported_after_the_held_ones() {
    let notified = entity(json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b"}
    }));
    let deleted = entity(json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 1}, {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:a"}],
        "name": {"type": "Property", "value": "bus"}
    }));
    let deleted_at = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("valid instant");
    let changes = NotifiedChanges {
        held: HeldAttributes::Current,
        deleted: Some(DeletedInstances {
            instances: &deleted,
            deleted_at: Some(deleted_at),
        }),
        previous: None,
    };
    let document = rendered(&notified, changes, ResponseFormat::Normalized);
    assert_eq!(document["speed"].as_array().map(Vec::len), Some(3), "{document}");
    assert_eq!(
        document["speed"][0],
        json!({"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b"})
    );
    assert_eq!(
        document["speed"][1],
        json!({"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2024-06-15T12:30:00Z"})
    );
    assert_eq!(document["speed"][2]["datasetId"], json!("urn:ngsi-ld:Dataset:a"));
    assert_eq!(document["speed"][2]["value"], json!("urn:ngsi-ld:null"));
    assert_eq!(
        document["name"],
        json!({"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2024-06-15T12:30:00Z"})
    );
}

/// The previous rendering holds only what the change touched, so the
/// instance of an untouched dataset gains no previous member.
#[test]
fn show_changes_marks_only_the_instances_the_change_touched() {
    let notified = entity(json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10},
            {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:Dataset:a"},
            {"type": "Property", "value": 30, "datasetId": "urn:ngsi-ld:Dataset:b"}
        ],
        "brand": {"type": "Property", "value": "x"}
    }));
    let previous = entity(json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 1}, {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b"}]
    }));
    let changes = NotifiedChanges {
        held: HeldAttributes::Current,
        deleted: None,
        previous: Some(&previous),
    };
    let document = rendered(&notified, changes, ResponseFormat::Normalized);
    assert_eq!(document["speed"][0]["previousValue"], json!(1));
    assert!(document["speed"][1].get("previousValue").is_none(), "{document}");
    assert_eq!(document["speed"][2]["previousValue"], json!(3));
    assert!(document["brand"].get("previousValue").is_none(), "{document}");
}

/// Clause 5.6.19 lets a change replace an attribute's family, and each family
/// prohibits the others' previous members (clauses 4.5.2.2 and 4.5.21.2): a
/// previous value spelled in another family's member is not reported.
#[test]
fn a_previous_value_of_another_family_is_not_reported() {
    let notified = entity(json!({"id": "urn:ngsi-ld:A:1", "type": "A", "readings": {"type": "ListProperty", "valueList": [3, 4]}}));
    let previous = entity(json!({"id": "urn:ngsi-ld:A:1", "type": "A", "readings": {"type": "Property", "value": 1}}));
    let changes = NotifiedChanges {
        held: HeldAttributes::Current,
        deleted: None,
        previous: Some(&previous),
    };
    let document = rendered(&notified, changes, ResponseFormat::Normalized);
    assert_eq!(document["readings"], json!({"type": "ListProperty", "valueList": [3, 4]}));
}

/// Clause 4.5.22.2: `previousObjectList` has the representation of
/// `objectList`, so the concise representation writes bare URIs for both.
#[test]
fn a_concise_previous_object_list_holds_bare_uris() {
    let notified = entity(json!({"id": "urn:ngsi-ld:A:1", "type": "A", "rooms": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:3"]}}));
    let previous = entity(
        json!({"id": "urn:ngsi-ld:A:1", "type": "A", "rooms": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]}}),
    );
    let changes = NotifiedChanges {
        held: HeldAttributes::Current,
        deleted: None,
        previous: Some(&previous),
    };
    let document = rendered(&notified, changes, ResponseFormat::Concise);
    assert_eq!(
        document["rooms"],
        json!({"objectList": ["urn:ngsi-ld:Room:3"], "previousObjectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]})
    );
}

/// A member written concisely is the family it abbreviates, and is reported
/// deleted in that family's value member, in the representation asked for.
#[test]
fn a_concise_member_is_nullified_in_the_value_member_of_its_family() {
    let notified = entity(json!({"id": "urn:ngsi-ld:A:1", "type": "A", "conciseLink": {"object": "urn:ngsi-ld:B:1"}}));
    let document = rendered(&notified, entity_deletion(None), ResponseFormat::Normalized);
    assert_eq!(document["conciseLink"], json!({"type": "Relationship", "object": "urn:ngsi-ld:null"}));
}

/// Clause 5.8.6: a member reported deleted by the bare NGSI-LD Null is
/// written as that Null in every representation.
#[test]
fn a_member_reported_by_the_bare_null_is_written_bare() {
    let notified = entity(json!({"id": "urn:ngsi-ld:A:1", "type": "A", "owner": "urn:ngsi-ld:null"}));
    for format in [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified] {
        let document = rendered(&notified, entity_deletion(None), format);
        assert_eq!(document["owner"], json!("urn:ngsi-ld:null"), "{format:?}");
    }
}

/// Clause 4.21 narrows `type` and `scope` of a notified entity; clause 5.8.6
/// keeps its `id`, without which an Entity is not actionable.
#[test]
fn a_projection_narrows_the_structural_members_but_never_the_id() {
    let notified = every_family();
    let projection = Projection {
        pick: vec![ProjectionTerm::Scope],
        omit: Vec::new(),
    };
    let stamped = StampedInstants::default();
    let written = NotificationEntity::new(&notified, &stamped, ResponseFormat::Normalized).with_projection(StructuralProjection::from(&projection));
    let document = serde_json::to_value(written).expect("the walk writes it");
    assert_eq!(document["id"], json!("urn:ngsi-ld:Vehicle:1"));
    assert!(document.get("type").is_none(), "{document}");
    assert_eq!(document["scope"], json!("/Madrid"));
}
