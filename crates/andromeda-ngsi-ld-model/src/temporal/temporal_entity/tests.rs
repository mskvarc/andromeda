use crate::{
    attribute::instance::Attribute,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
    temporal::temporal_entity::TemporalEntity,
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Value, json, value::RawValue};
use std::convert::Infallible;

/// The Temporal Evolution a compact document is.
fn read(document: &Value) -> Result<TemporalEntity, NodeReadError<Infallible>> {
    let raw = RawValue::from_string(document.to_string()).expect("raw JSON");
    TemporalEntity::from_node(JsonPiece::new(&raw))
}

/// A vehicle holding `members`.
fn vehicle(members: Value) -> Value {
    let mut document = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"});
    if let (Some(target), Value::Object(members)) = (document.as_object_mut(), members) {
        target.extend(members);
    }
    document
}

/// Clause 4.5.7: an Attribute whose history is one instance is still the
/// sequence of that one instance.
#[test]
fn a_single_instance_attribute_is_held_as_a_list() {
    let entity = read(&vehicle(
        json!({"speed": {"type": "Property", "value": 50, "observedAt": "2020-01-01T00:00:00Z"}}),
    ))
    .expect("an entity");
    let speed = &entity.attrs["speed"];
    pretty_assertions::assert_eq!(speed.family(), AttributeType::Property);
    pretty_assertions::assert_eq!(speed.instances().len(), 1);
}

/// Clause 4.18 and annex C, clause C.5.16: the Scope history is read as the
/// Property instances it is.
#[test]
fn the_scope_history_is_read_as_property_instances() {
    let entity = read(&vehicle(json!({"scope": [
        {"type": "Property", "value": "/a", "observedAt": "2020-01-01T00:00:00Z"},
        {"type": "Property", "value": ["/a", "/b"], "observedAt": "2020-01-02T00:00:00Z"}
    ]})))
    .expect("an entity");
    pretty_assertions::assert_eq!(entity.scope.expect("a scope history").instances().len(), 2);
    assert!(entity.attrs.is_empty(), "the scope is no ordinary attribute");
}

/// Clause 4.5.0: a deleted instance is its family's deletion encoding beside a
/// `deletedAt`, read as the instance it is, for every family.
#[test]
fn a_deleted_instance_carries_its_deletion_instant() {
    let entity = read(&vehicle(json!({
        "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2020-01-03T00:00:00Z"}],
        "says": [{"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}, "deletedAt": "2020-01-03T00:00:00Z"}],
        "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:null", "deletedAt": "2020-01-03T00:00:00Z"}],
        "tags": [{"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"], "deletedAt": "2020-01-03T00:00:00Z"}]
    })))
    .expect("an entity");
    for (name, family) in [
        ("speed", AttributeType::Property),
        ("says", AttributeType::LanguageProperty),
        ("owner", AttributeType::Relationship),
        ("tags", AttributeType::ListProperty),
    ] {
        let attribute = &entity.attrs[name];
        pretty_assertions::assert_eq!(attribute.family(), family, "{name}");
        assert!(attribute.instances()[0].common().deleted_at.is_some(), "{name}");
    }
}

/// Clause 4.5.7: the `instanceId` and the system times of an instance are kept.
#[test]
fn instance_identifiers_and_system_times_are_kept() {
    let entity = read(&vehicle(json!({"speed": [{
        "type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z",
        "instanceId": "urn:ngsi-ld:Instance:1", "createdAt": "2020-01-01T00:00:01Z", "modifiedAt": "2020-01-01T00:00:02Z"
    }]})))
    .expect("an entity");
    let common = entity.attrs["speed"].instances()[0].common();
    assert!(common.instance_id.is_some() && common.created_at.is_some() && common.modified_at.is_some());
}

/// Clause 4.5.7: several instances of one `datasetId` are the sequence, not a
/// conflict.
#[test]
fn several_instances_share_a_dataset() {
    let instance = |at: &str| json!({"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a", "observedAt": at});
    let entity = read(&vehicle(json!({"speed": [
        instance("2020-01-01T00:00:00Z"), instance("2020-01-02T00:00:00Z"), instance("2020-01-03T00:00:00Z")
    ]})))
    .expect("an entity");
    pretty_assertions::assert_eq!(entity.attrs["speed"].instances().len(), 3);
    assert!(
        entity.attrs["speed"]
            .instances()
            .iter()
            .all(|instance| Attribute::dataset_id(instance).is_some())
    );
}

/// An empty array holds no instance and names no family: no attribute.
#[test]
fn an_attribute_without_an_instance_is_no_attribute() {
    let entity = read(&vehicle(json!({"speed": []}))).expect("an entity");
    assert!(entity.attrs.is_empty());
}

/// The entity's own system times and context are not attributes.
#[test]
fn the_entity_members_are_not_attributes() {
    let entity = read(&vehicle(json!({
        "@context": "https://example.org/ctx", "createdAt": "2020-01-01T00:00:00Z", "modifiedAt": "2020-01-02T00:00:00Z"
    })))
    .expect("an entity");
    assert!(entity.attrs.is_empty());
    assert!(entity.system_times.created_at.is_some() && entity.system_times.modified_at.is_some());
}

/// Clause 4.5.1: an evolution without `type` is refused, naming it.
#[test]
fn an_entity_without_a_type_is_refused() {
    let Err(NodeReadError::Refused(refused)) = read(&json!({"id": "urn:ngsi-ld:Vehicle:1"})) else {
        panic!("the entity is refused");
    };
    assert!(matches!(refused.refusal, NodeRefusal::Missing { member: RefusedMember::Type }));
}

/// An instance without the member its family holds its value in is refused,
/// where it used to render as a Property with a null value.
#[test]
fn an_instance_without_a_family_member_is_refused() {
    let refused = read(&vehicle(json!({"speed": [{"type": "Property", "observedAt": "2020-01-01T00:00:00Z"}]})));
    assert!(matches!(refused, Err(NodeReadError::Refused(_))));
}
