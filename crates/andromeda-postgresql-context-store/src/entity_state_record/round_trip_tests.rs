//! A committed state and its record give back the same state, a record that
//! breaks the stored layout is refused, and an instance record holds a
//! `current_attributes` row field for field.

use crate::{
    current_attribute::{
        round_trip_tests::{committed, date_time, dictionary, every_family, ex, nested_every_family, ngsi, property, with},
        row::CurrentAttributeRow,
    },
    entity::stored_entity_error::StoredEntityError,
    entity_state_record::{
        instance_record::AttributeInstanceRecord,
        state_record::{EntityStateRecord, record_of},
        stored_state::StoredState,
    },
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_attribute_instance::{
    name_keys::KeyNames,
    stored_json::StoredJson,
    stored_members::InstanceMembers,
    stored_value::{LiteralAnnotation, StoredValue},
    sub_attribute::SubAttributeElement,
};
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use cefact_units::UnitCode;
use chrono::{TimeZone, Utc};
use serde_json::{Value, json, value::to_raw_value};

/// The identifier every fixture state carries.
fn building_id() -> Identifier {
    "urn:ngsi-ld:Building:1".parse().expect("an identifier")
}

/// A state with the entity members `head` and the attributes `attributes`.
fn state(head: &Value, attributes: &Value) -> EntityState {
    committed(&json!([with(with(json!({"@id": "urn:ngsi-ld:Building:1"}), head), attributes)]))
}

/// The state `record` holds, every key read back through `names`.
fn state_of(record: EntityStateRecord, names: &KeyNames) -> Result<EntityState, StoredEntityError> {
    StoredState::try_from(record)?.assemble(&building_id(), names)
}

/// The record of the type-only Building, through its own dictionary.
fn bare_record() -> (EntityStateRecord, KeyNames) {
    let bare = state(&json!({"@type": [ex("Building")]}), &json!({}));
    let (keys, names) = dictionary(&bare);
    (record_of(&bare, &keys).expect("the state has a record"), names)
}

#[test]
fn a_state_round_trips_through_its_record() {
    let mut attributes = every_family(&nested_every_family());
    if let Some(members) = attributes.as_object_mut() {
        members.insert(
            ex("temperature"),
            json!([
                property(&json!([{"@value": 20}])),
                with(
                    property(&json!([{"@value": 21}])),
                    &json!({ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:b"}]})
                )
            ]),
        );
    }
    let written = state(
        &json!({
            "@type": [ex("B"), ex("A")],
            ngsi("scope"): [{"@value": "/Madrid/Centro"}, {"@value": "/x"}],
            ngsi("expiresAt"): date_time("2030-01-01T00:00:00.25Z")
        }),
        &attributes,
    );
    let (keys, names) = dictionary(&written);
    let record = record_of(&written, &keys).expect("a committed state has a record");
    pretty_assertions::assert_eq!(record.scope_paths, vec!["/Madrid/Centro".to_owned(), "/x".to_owned()]);
    pretty_assertions::assert_eq!(record.instances.len(), 10, "eight attributes, and temperature twice");
    pretty_assertions::assert_eq!(state_of(record, &names).expect("the record reads back"), written);
}

#[test]
fn a_state_without_attributes_round_trips() {
    let bare = state(&json!({"@type": [ex("Building")]}), &json!({}));
    let (keys, names) = dictionary(&bare);
    let record = record_of(&bare, &keys).expect("the state has a record");
    assert!(record.instances.is_empty() && record.scope_paths.is_empty() && record.expires_at.is_none());
    pretty_assertions::assert_eq!(state_of(record, &names).expect("the record reads back"), bare);
}

/// Clause 4.5.1 gives every Entity a type.
#[test]
fn a_record_without_types_is_refused() {
    let (mut record, names) = bare_record();
    record.entity_type_name_keys = Vec::new();
    assert!(matches!(state_of(record, &names), Err(StoredEntityError::Untyped)));
}

#[test]
fn a_type_key_without_a_name_is_refused() {
    let (mut record, names) = bare_record();
    record.entity_type_name_keys = vec![99];
    assert!(matches!(state_of(record, &names), Err(StoredEntityError::TypeKey(key)) if key == EntityTypeNameKey::from(99)));
}

/// Clause 4.18: a Scope is a path of levels under `/`.
#[test]
fn a_scope_that_is_no_scope_path_is_refused() {
    let (mut record, names) = bare_record();
    record.scope_paths = vec!["not a path".to_owned()];
    assert!(matches!(state_of(record, &names), Err(StoredEntityError::Scope(_))));
}

/// `row` through its record and back.
fn through(row: CurrentAttributeRow) -> CurrentAttributeRow {
    CurrentAttributeRow::try_from(AttributeInstanceRecord::try_from(row).expect("the row encodes")).expect("the record decodes")
}

/// A row holding `value` and no other member.
fn row_of(value: StoredValue) -> CurrentAttributeRow {
    let instant = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).single().expect("a valid instant");
    CurrentAttributeRow {
        ordinal: 0,
        members: InstanceMembers {
            attribute_name_key: AttributeNameKey::from(7),
            dataset_id: None,
            attribute_kind: AttributeKind::Property,
            value,
            value_type_key: None,
            object_type_keys: Vec::new(),
            unit_code: None,
            observed_at: None,
            expires_at: None,
        },
        created_at: instant,
        modified_at: instant,
        sub_attributes: Vec::new(),
    }
}

/// A `GeoProperty` row with every optional member and one sub-attribute.
fn full_row() -> CurrentAttributeRow {
    let instant = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).single().expect("a valid instant");
    let json = |value: Value| StoredJson::new(to_raw_value(&value).expect("JSON"));
    let mut row = row_of(StoredValue::Geometry {
        form: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.5, 52.25, 34.0]}))
            .expect("a point")
            .into_parts()
            .0,
        members: Some(json(json!({"https://ex.org/title": [{"@value": "x"}]}))),
    });
    row.ordinal = 4;
    row.members.dataset_id = Some("urn:ngsi-ld:Dataset:a".parse().expect("a datasetId"));
    row.members.attribute_kind = AttributeKind::GeoProperty;
    row.members.value_type_key = Some(EntityTypeNameKey::from(3));
    row.members.object_type_keys = vec![EntityTypeNameKey::from(4), EntityTypeNameKey::from(5)];
    row.members.unit_code = Some(UnitCode::Kgm);
    row.members.observed_at = Some(instant);
    row.members.expires_at = Some(instant);
    row.sub_attributes = vec![SubAttributeElement {
        parent: 0,
        members: row_of(StoredValue::String {
            text: "sub".to_owned(),
            annotation: LiteralAnnotation::Language("en".parse().expect("a language tag")),
        })
        .members,
        created_at: Some(instant),
        modified_at: None,
    }];
    row
}

#[test]
fn an_instance_record_is_a_current_row_field_for_field() {
    let full = full_row();
    let full_debug = format!("{full:?}");
    pretty_assertions::assert_eq!(format!("{:?}", through(full)), full_debug);
    let empty = || row_of(StoredValue::Boolean { flag: false, datatype: None });
    pretty_assertions::assert_eq!(through(empty()), empty());
    let record = AttributeInstanceRecord::try_from(empty()).expect("the row encodes");
    assert!(
        record.value_number.is_none()
            && record.value_string.is_none()
            && record.value_long_text.is_none()
            && record.value_date_time.is_none()
            && record.value_geometry.is_none()
            && record.value_geometry_members.is_none()
            && record.value_object.is_none()
            && record.value_uri.is_none()
            && record.value_long_uri.is_none()
            && record.value_datatype_key.is_none()
            && record.value_language.is_none()
            && record.value_type_key.is_none()
            && record.object_type_keys.is_none()
            && record.unit_code_id.is_none()
            && record.observed_at.is_none()
            && record.expires_at.is_none()
            && record.dataset_key.is_empty()
            && record.sub_attributes.is_none(),
        "{record:?}"
    );
    pretty_assertions::assert_eq!(record.value_boolean, Some(false));
}

/// A String or a URI past the `current_attributes` B-tree bound sits in its
/// overflow field, as the row's own column does.
#[test]
fn an_instance_record_keeps_the_length_split_of_its_row() {
    let text = row_of(StoredValue::String {
        text: "x".repeat(3000),
        annotation: LiteralAnnotation::Plain,
    });
    let record = AttributeInstanceRecord::try_from(row_of(StoredValue::String {
        text: "x".repeat(3000),
        annotation: LiteralAnnotation::Plain,
    }))
    .expect("the row encodes");
    assert!(record.value_string.is_none() && record.value_long_text.is_some());
    pretty_assertions::assert_eq!(
        through(row_of(StoredValue::String {
            text: "x".repeat(3000),
            annotation: LiteralAnnotation::Plain,
        })),
        text
    );
    let uri = || row_of(StoredValue::Uri(format!("urn:ngsi-ld:Thing:{}", "y".repeat(1100)).parse().expect("a URI")));
    let record = AttributeInstanceRecord::try_from(uri()).expect("the row encodes");
    assert!(record.value_uri.is_none() && record.value_long_uri.is_some());
    pretty_assertions::assert_eq!(through(uri()), uri());
}

#[test]
fn a_record_holding_a_string_on_both_sides_of_the_split_is_refused() {
    let mut record = AttributeInstanceRecord::try_from(row_of(StoredValue::String {
        text: "short".to_owned(),
        annotation: LiteralAnnotation::Plain,
    }))
    .expect("the row encodes");
    record.value_long_text = Some("x".repeat(3000));
    assert!(CurrentAttributeRow::try_from(record).is_err());
}
