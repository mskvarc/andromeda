use crate::temporal_entry_record::{TemporalEntryRecord, TemporalIntentRecord, mention_intent};
use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    scope::path::ScopePath,
};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue, observation_metadata::ObservationMetadata};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_attribute_instance::{
    name_keys::{MentionedNames, NameKeys},
    name_resolution::distinct,
};
use andromeda_postgresql_codes::{
    attribute_kind::AttributeKind,
    scope_value_kind::ScopeValueKind,
    temporal_entry_kind::TemporalEntryKind,
    value_kind::ValueKind,
};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
use andromeda_postgresql_schema::{temporal::AttributeHistory, temporal_intent::TemporalEntry};
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    scope_observation::ScopeObservation,
};
use bigdecimal::BigDecimal;
use chrono::{DateTime, TimeZone, Utc};
use pretty_assertions::assert_eq;
use sea_query::Iden;
use serde_json::json;
use uuid::Uuid;

const SPEED: &str = "https://example.com/speed";
const VEHICLE: &str = "https://example.com/Vehicle";

pub(crate) fn recorded_at() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")
}

fn observed_at() -> DateTime<Utc> {
    Utc.timestamp_opt(1_699_999_000, 0).single().expect("a representable instant")
}

/// One intent over `urn:ngsi-ld:Vehicle:1`, a Vehicle, carrying `entries`.
pub(crate) fn intent(entries: Vec<TemporalAppendEntry>) -> TemporalAppendIntent {
    TemporalAppendIntent {
        record: WriteRecord::restored(WriteId::mint(), recorded_at()),
        base_ordinal: WriteOrdinal::at(4),
        entity_id: "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity URI"),
        entity_types: vec![VEHICLE.parse::<EntityTypeName>().expect("an entity type")],
        entries,
    }
}

/// Keys for every name `intent` mentions, numbered from 1 in sorted order.
pub(crate) fn keys(intent: &TemporalAppendIntent) -> NameKeys {
    let mut mentioned = MentionedNames::default();
    mention_intent(intent, &mut mentioned);
    let MentionedNames { attributes, types } = distinct(mentioned);
    NameKeys::new(
        ResolvedNames::from_pairs(attributes.into_iter().zip(1..).map(|(name, key)| (name, AttributeNameKey::from(key)))),
        ResolvedNames::from_pairs(types.into_iter().zip(1..).map(|(name, key)| (name, EntityTypeNameKey::from(key)))),
    )
}

/// The one entry `intent` is enqueued with.
fn only_entry(intent: &TemporalAppendIntent) -> TemporalEntryRecord {
    let TemporalIntentRecord { mut entries, .. } = TemporalIntentRecord::of(intent, &keys(intent)).expect("the intent is storable");
    assert_eq!(entries.len(), 1);
    entries.remove(0)
}

/// The metadata of a `speed` Property holding the unit `KMH` and an
/// `accuracy` sub-attribute.
fn unit_and_accuracy() -> ObservationMetadata {
    let speed: AttributeName = SPEED.parse().expect("an attribute name");
    let instance = json!({
        "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}],
        "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KMH"}],
        "https://example.com/accuracy": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.9}]
        }]
    });
    let document = InstanceDocument::from_slice(&speed, &serde_json::to_vec(&instance).expect("JSON")).expect("expanded members");
    ObservationMetadata::admit(document.into_parts())
        .expect("the members are committed")
        .expect("the members are metadata")
}

/// An observation of `speed` holding `value`, recorded as `instance`.
pub(crate) fn observation(value: TemporalValue, instance: InstanceIdentity, metadata: Option<ObservationMetadata>) -> TemporalAppendEntry {
    TemporalAppendEntry::Observation {
        row: TemporalHistoryRow {
            attribute_name: SPEED.parse().expect("an attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value,
            observed_at: Some(observed_at()),
            recorded_at: recorded_at(),
            created_at: Some(recorded_at()),
            modified_at: Some(recorded_at()),
            instance,
            metadata,
            partition_total: None,
        },
    }
}

#[test]
fn an_observation_entry_holds_the_history_columns_it_will_be_written_as() {
    let generated = Uuid::from_u128(7);
    let entry = only_entry(&intent(vec![observation(
        TemporalValue::Number(JsonNumber::from(42)),
        InstanceIdentity::Generated(generated),
        Some(unit_and_accuracy()),
    )]));
    assert_eq!(entry.entry_kind, TemporalEntryKind::Observation.code());
    assert_eq!(entry.value_kind, ValueKind::Number.code());
    assert_eq!(entry.attribute_kind, Some(AttributeKind::Property.code()));
    assert_eq!(entry.value_number, Some(BigDecimal::from(42)));
    assert_eq!(entry.observed_at, Some(observed_at()));
    assert_eq!(entry.created_at, Some(recorded_at()));
    assert!(entry.unit_code_id.is_some());
    assert_eq!(entry.sub_attributes.as_ref().map(Vec::len), Some(1));
    assert_eq!(entry.generated_instance_id, Some(generated));
    assert_eq!(entry.verbatim_instance_id, None);
    assert!(entry.attribute_name_key.is_some());
    assert!(entry.deleted_at.is_none() && entry.value_string.is_none() && entry.value_object.is_none());
}

/// Clause 4.5.7 hands a client-supplied `instanceId` back to the client
/// exactly as it was written.
#[test]
fn a_client_instance_id_is_held_verbatim() {
    let entry = only_entry(&intent(vec![observation(
        TemporalValue::Number(JsonNumber::from(1)),
        InstanceIdentity::from_identifier("urn:x".parse().expect("a URI"), InstanceFamily::Attribute),
        None,
    )]));
    assert_eq!(entry.verbatim_instance_id.as_deref(), Some("urn:x"));
    assert_eq!(entry.generated_instance_id, None);
    assert!(entry.sub_attributes.is_none() && entry.unit_code_id.is_none());
}

/// Clause 4.5.8: a deleted Relationship is recorded as an instance of the
/// Relationship with `deletedAt` set, so the end keeps its family and the
/// write's instant, and no value.
#[test]
fn a_tombstone_entry_holds_its_family_and_deletion_instant() {
    let entry = only_entry(&intent(vec![TemporalAppendEntry::Tombstone {
        attribute_name: "https://example.com/owner".parse().expect("an attribute name"),
        dataset_id: None,
        attribute_type: AttributeType::Relationship,
        instance: InstanceIdentity::generate(),
    }]));
    assert_eq!(entry.entry_kind, TemporalEntryKind::Tombstone.code());
    assert_eq!(entry.value_kind, ValueKind::Tombstone.code());
    assert_eq!(entry.attribute_kind, Some(AttributeKind::Relationship.code()));
    assert_eq!(entry.deleted_at, Some(recorded_at()));
    assert!(entry.value_uri.is_none() && entry.observed_at.is_none() && entry.created_at.is_none());
}

/// Clause 4.18: a Scope is stored as its label path beside the Scope as
/// written, and names no attribute stream.
#[test]
fn a_scope_entry_holds_its_path() {
    let entry = only_entry(&intent(vec![TemporalAppendEntry::Scope {
        observation: ScopeObservation::Path("/Madrid".parse::<ScopePath>().expect("a scope path")),
        observed_at: Some(observed_at()),
        instance: InstanceIdentity::generate(),
    }]));
    assert_eq!(entry.entry_kind, TemporalEntryKind::Scope.code());
    assert_eq!(entry.value_kind, ScopeValueKind::ScopePath.code());
    assert_eq!(entry.scope_path.as_deref(), Some("Madrid"));
    assert_eq!(entry.verbatim_scope_path.as_deref(), Some("/Madrid"));
    assert_eq!((entry.attribute_name_key, entry.attribute_kind), (None, None));
    assert_eq!(entry.observed_at, Some(observed_at()));
    assert_eq!(entry.deleted_at, None);
}

/// Clause 4.18 reserves the NGSI-LD Null for a deleted Scope, recorded as the
/// deletion instant and no path.
#[test]
fn a_scope_deletion_holds_no_path() {
    let entry = only_entry(&intent(vec![TemporalAppendEntry::Scope {
        observation: ScopeObservation::Deleted,
        observed_at: Some(recorded_at()),
        instance: InstanceIdentity::generate(),
    }]));
    assert_eq!(entry.entry_kind, TemporalEntryKind::Scope.code());
    assert_eq!(entry.value_kind, ScopeValueKind::Tombstone.code());
    assert_eq!((entry.scope_path, entry.verbatim_scope_path), (None, None));
    assert_eq!(entry.deleted_at, Some(recorded_at()));
}

/// Clauses 4.5.7 and 4.5.8 record an ending in an evolution that exists, so
/// an intent that only ends instances neither creates one nor replaces the
/// types of one, whatever types the write stated.
#[test]
fn a_deletion_intent_carries_no_types() {
    let deletion = intent(vec![TemporalAppendEntry::Tombstone {
        attribute_name: SPEED.parse().expect("an attribute name"),
        dataset_id: None,
        attribute_type: AttributeType::Property,
        instance: InstanceIdentity::generate(),
    }]);
    let record = TemporalIntentRecord::of(&deletion, &keys(&deletion)).expect("the intent is storable");
    assert_eq!(record.entity_type_name_keys, Vec::<i32>::new());
    assert_eq!(record.base_ordinal, 4);

    let observing = intent(vec![observation(TemporalValue::Boolean(true), InstanceIdentity::generate(), None)]);
    let record = TemporalIntentRecord::of(&observing, &keys(&observing)).expect("the intent is storable");
    assert_eq!(record.entity_type_name_keys, vec![1]);
}

#[test]
fn the_entry_fields_follow_the_history_columns() {
    let fields = [
        TemporalEntry::EntryKind,
        TemporalEntry::AttributeNameKey,
        TemporalEntry::DatasetId,
        TemporalEntry::AttributeKind,
        TemporalEntry::ValueKind,
        TemporalEntry::ObservedAt,
        TemporalEntry::CreatedAt,
        TemporalEntry::ModifiedAt,
        TemporalEntry::DeletedAt,
        TemporalEntry::ValueDateTime,
        TemporalEntry::ExpiresAt,
        TemporalEntry::GeneratedInstanceId,
        TemporalEntry::ValueTypeKey,
        TemporalEntry::UnitCodeId,
        TemporalEntry::ValueBoolean,
        TemporalEntry::VerbatimInstanceId,
        TemporalEntry::ValueNumber,
        TemporalEntry::ValueString,
        TemporalEntry::ValueGeometry,
        TemporalEntry::ValueGeometryMembers,
        TemporalEntry::ValueObject,
        TemporalEntry::ValueUri,
        TemporalEntry::ObjectTypeKeys,
        TemporalEntry::ProofSeals,
        TemporalEntry::SubAttributes,
        TemporalEntry::ScopePath,
        TemporalEntry::VerbatimScopePath,
    ]
    .map(|field| field.to_string());
    // `attribute_history` in its column order, but the write identity and the
    // stream, which the queue row and the catalogue supply.
    let history_columns = [
        AttributeHistory::AttributeKind,
        AttributeHistory::ValueKind,
        AttributeHistory::ObservedAt,
        AttributeHistory::CreatedAt,
        AttributeHistory::ModifiedAt,
        AttributeHistory::DeletedAt,
        AttributeHistory::ValueDateTime,
        AttributeHistory::ExpiresAt,
        AttributeHistory::GeneratedInstanceId,
        AttributeHistory::ValueTypeKey,
        AttributeHistory::UnitCodeId,
        AttributeHistory::ValueBoolean,
        AttributeHistory::VerbatimInstanceId,
        AttributeHistory::ValueNumber,
        AttributeHistory::ValueString,
        AttributeHistory::ValueGeometry,
        AttributeHistory::ValueGeometryMembers,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueUri,
        AttributeHistory::ObjectTypeKeys,
        AttributeHistory::ProofSeals,
        AttributeHistory::SubAttributes,
    ]
    .map(|column| column.to_string());
    let expected: Vec<String> = ["entry_kind", "attribute_name_key", "dataset_id"]
        .into_iter()
        .map(String::from)
        .chain(history_columns)
        .chain(["scope_path", "verbatim_scope_path"].map(String::from))
        .collect();
    assert_eq!(TemporalEntry::Type.to_string(), "temporal_entry");
    assert_eq!(fields.to_vec(), expected);
}
