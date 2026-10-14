//! One temporal append intent as the `temporal_intent` composite a commit
//! enqueues it as, and each of its entries as the `temporal_entry` it waits in.
//!
//! An entry holds exactly the columns its history row will hold: the value as
//! [`AttributeHistoryValue`] stores it, the metadata as [`HistoryMembers`]
//! stores it, every name as its dictionary key. Applying an intent therefore
//! moves its entries into history in SQL, and nothing is read back into the
//! broker on the way.

use crate::{
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    history_members::{HistoryMembers, mention},
    scope_history_value::ScopeHistoryValue,
    semantic_time_axes::SemanticTimeAxes,
    temporal_entry_record::entry_fields::{Grain, MemberColumns, ValueColumns, record},
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_postgresql_attribute_instance::{
    name_keys::{MentionedNames, NameKeys},
    sub_attribute::{EwkbGeometry, SubAttributeRecord},
};
use andromeda_postgresql_codes::{
    attribute_kind_code::stored_attribute_kind,
    temporal_entry_kind::TemporalEntryKind,
    temporal_intent_reach::TemporalIntentReach,
};
use andromeda_postgresql_query::{entity_type_name_key::EntityTypeNameKey, ltree::StoredScopePath};
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    scope_observation::ScopeObservation,
    write_purpose::TemporalWritePurpose,
};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::types::Json;
use uuid::Uuid;

/// The `temporal_entry` composite, field for field in declaration order.
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "temporal_entry")]
pub struct TemporalEntryRecord {
    /// `entry_kind`.
    pub entry_kind: i16,
    /// `attribute_name_key`, NULL for a Scope entry.
    pub attribute_name_key: Option<i32>,
    /// `dataset_id`.
    pub dataset_id: Option<String>,
    /// `attribute_kind`, NULL for a Scope entry.
    pub attribute_kind: Option<i16>,
    /// `value_kind`, the `scope_history` code for a Scope entry.
    pub value_kind: i16,
    /// `observed_at`.
    pub observed_at: Option<DateTime<Utc>>,
    /// `created_at`.
    pub created_at: Option<DateTime<Utc>>,
    /// `modified_at`.
    pub modified_at: Option<DateTime<Utc>>,
    /// `deleted_at`.
    pub deleted_at: Option<DateTime<Utc>>,
    /// `value_date_time`.
    pub value_date_time: Option<DateTime<Utc>>,
    /// `expires_at`.
    pub expires_at: Option<DateTime<Utc>>,
    /// `generated_instance_id`.
    pub generated_instance_id: Option<Uuid>,
    /// `value_type_key`.
    pub value_type_key: Option<i32>,
    /// `unit_code_id`.
    pub unit_code_id: Option<i16>,
    /// `value_boolean`.
    pub value_boolean: Option<bool>,
    /// `verbatim_instance_id`.
    pub verbatim_instance_id: Option<String>,
    /// `value_number`.
    pub value_number: Option<BigDecimal>,
    /// `value_string`.
    pub value_string: Option<String>,
    /// `value_geometry`.
    pub value_geometry: Option<EwkbGeometry>,
    /// `value_geometry_members`.
    pub value_geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    pub value_object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    pub value_uri: Option<String>,
    /// `object_type_keys`, NULL when the list is empty.
    pub object_type_keys: Option<Vec<i32>>,
    /// `proof_seals`.
    pub proof_seals: Option<Json<Box<RawValue>>>,
    /// `sub_attributes`, NULL when the instance has none.
    pub sub_attributes: Option<Vec<SubAttributeRecord>>,
    /// `scope_path`, the `ltree` label path of a Scope entry.
    pub scope_path: Option<String>,
    /// `verbatim_scope_path`, the Scope as written.
    pub verbatim_scope_path: Option<String>,
}

/// The `temporal_intent` composite, field for field in declaration order.
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "temporal_intent")]
pub struct TemporalIntentRecord {
    /// `entity_id`.
    pub entity_id: String,
    /// `entity_type_name_keys`, empty for an intent that only ends instances.
    pub entity_type_name_keys: Vec<i32>,
    /// `base_ordinal`.
    pub base_ordinal: i32,
    /// `entries`, in write order.
    pub entries: Vec<TemporalEntryRecord>,
    /// `entry_needs`, the [`TemporalIntentReach`] codes the intent meets, in
    /// registry order.
    pub entry_needs: Vec<i16>,
}

/// Add every name `intent` holds as a key to `mentioned`: its types when it
/// may bring an evolution into being, each entry's attribute name, and the
/// names its observations' metadata holds.
pub fn mention_intent(intent: &TemporalAppendIntent, mentioned: &mut MentionedNames) {
    match intent.write_purpose() {
        TemporalWritePurpose::Observation => mentioned.types.extend(intent.entity_types.iter().cloned()), // clone: the dictionary request owns the names it resolves
        TemporalWritePurpose::Deletion => {}
    }
    for entry in &intent.entries {
        match entry {
            TemporalAppendEntry::Observation { row } => {
                mentioned.attributes.push(row.attribute_name.clone()); // clone: the dictionary request owns the names it resolves
                if let Some(metadata) = &row.metadata {
                    mention(metadata, mentioned);
                }
            }
            TemporalAppendEntry::Tombstone { attribute_name, .. } => mentioned.attributes.push(attribute_name.clone()), // clone: the dictionary request owns the names it resolves
            TemporalAppendEntry::Scope { .. } => {}
        }
    }
}

impl TemporalIntentRecord {
    /// The record `intent` is enqueued as, every name held as its key in
    /// `keys`.
    ///
    /// An intent that only ends instances carries no types: clauses 4.5.7 and
    /// 4.5.8 record an ending in an evolution that exists, so its types
    /// neither create one nor replace the types of one.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError`] when a name has no key in `keys`, an
    /// entry's ordinal or value cannot be stored, or a sub-attribute does not
    /// flatten.
    pub fn of(intent: &TemporalAppendIntent, keys: &NameKeys) -> Result<TemporalIntentRecord, TemporalStoreError> {
        let entity_type_name_keys = match intent.write_purpose() {
            TemporalWritePurpose::Observation => intent
                .entity_types
                .iter()
                .map(|entity_type| {
                    keys.entity_type(entity_type)
                        .map(EntityTypeNameKey::get)
                        // clone: the invariant owns the type the dictionary failed to resolve.
                        .ok_or_else(|| TemporalStoreInvariant::EntityTypeUnresolved(entity_type.clone()).into())
                })
                .collect::<Result<Vec<_>, TemporalStoreError>>()?,
            TemporalWritePurpose::Deletion => Vec::new(),
        };
        let recorded_at = intent.record.recorded_at();
        let entries = intent
            .entries
            .iter()
            .map(|entry| entry_record(entry, recorded_at, keys))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TemporalIntentRecord::reaching(
            String::from(intent.entity_id.as_str()),
            entity_type_name_keys,
            intent.base_ordinal.as_i32().map_err(TemporalStoreError::WriteOrdinal)?,
            entries,
        ))
    }

    /// The record of `entries`, with the needs they meet.
    pub(crate) fn reaching(entity_id: String, entity_type_name_keys: Vec<i32>, base_ordinal: i32, entries: Vec<TemporalEntryRecord>) -> TemporalIntentRecord {
        let entry_needs = TemporalIntentReach::ALL
            .iter()
            .filter(|need| meets(&entity_type_name_keys, &entries, **need))
            .map(|need| need.code())
            .collect();
        TemporalIntentRecord {
            entity_id,
            entity_type_name_keys,
            base_ordinal,
            entries,
            entry_needs,
        }
    }

    /// Whether the intent meets `need`.
    pub(crate) fn reaches(&self, need: TemporalIntentReach) -> bool {
        self.entry_needs.contains(&need.code())
    }

    /// The intent as a statement that reads none of its entries binds it:
    /// every entry left out, so the server decodes no value.
    pub(crate) fn head(&self) -> TemporalIntentRecord {
        TemporalIntentRecord {
            // clone: the bound head owns its identifier, types and needs, one of each per intent.
            entity_id: self.entity_id.clone(),
            entity_type_name_keys: self.entity_type_name_keys.clone(),
            base_ordinal: self.base_ordinal,
            entries: Vec::new(),
            entry_needs: self.entry_needs.clone(),
        }
    }

    /// The intent as a statement that reads only what identifies each entry
    /// binds it: its kind, its grain and its instance identifiers.
    pub(crate) fn identities(&self) -> TemporalIntentRecord {
        TemporalIntentRecord {
            entries: self.entries.iter().map(TemporalEntryRecord::identity).collect(),
            ..self.head()
        }
    }
}

/// Whether an intent stating `entity_type_name_keys` and holding `entries`
/// meets `need`.
///
/// An intent observes when it holds no entry or any entry that does not end
/// an instance: clause 4.5.1 requires only `id` and `type` of an Entity, so a
/// write observing one with no attribute still extends its evolution.
fn meets(entity_type_name_keys: &[i32], entries: &[TemporalEntryRecord], need: TemporalIntentReach) -> bool {
    let is_attribute = |entry: &TemporalEntryRecord| entry.entry_kind != TemporalEntryKind::Scope.code();
    match need {
        TemporalIntentReach::TypedIntent => !entity_type_name_keys.is_empty(),
        TemporalIntentReach::UntypedObservation => {
            entity_type_name_keys.is_empty() && (entries.is_empty() || entries.iter().any(|entry| entry.entry_kind != TemporalEntryKind::Tombstone.code()))
        }
        TemporalIntentReach::AttributeEntry => entries.iter().any(is_attribute),
        TemporalIntentReach::VerbatimInstance => entries.iter().any(|entry| is_attribute(entry) && entry.verbatim_instance_id.is_some()),
        TemporalIntentReach::ScopeEntry => !entries.iter().all(is_attribute),
    }
}

/// The record one entry waits in, its instants those of the write accepted at
/// `recorded_at`.
fn entry_record(entry: &TemporalAppendEntry, recorded_at: DateTime<Utc>, keys: &NameKeys) -> Result<TemporalEntryRecord, TemporalStoreError> {
    match entry {
        TemporalAppendEntry::Observation { row } => {
            let members = row
                .metadata
                .as_ref()
                .map(|metadata| HistoryMembers::keyed(metadata, keys))
                .transpose()?
                .unwrap_or_default();
            observation_record(row, members, keys)
        }
        TemporalAppendEntry::Tombstone {
            attribute_name,
            dataset_id,
            attribute_type,
            instance,
        } => Ok(record(
            // The family the ended instance belonged to is what the render
            // synthesises the null member of (clauses 4.5.7 and 4.5.8).
            Grain::attribute(
                TemporalEntryKind::Tombstone,
                attribute_name,
                dataset_id.as_ref(),
                stored_attribute_kind(*attribute_type),
                keys,
            )?,
            ValueColumns::of_attribute(AttributeHistoryValue::Tombstone { deleted_at: recorded_at })?,
            MemberColumns::of(HistoryMembers::default())?,
            SemanticTimeAxes::default(),
            instance,
        )),
        TemporalAppendEntry::Scope {
            observation,
            observed_at,
            instance,
        } => {
            let value = match observation {
                // clone: the entry owns its validated path.
                ScopeObservation::Path(path) => ScopeHistoryValue::Path(path.clone()),
                // Clause 4.18 reserves `urn:ngsi-ld:null` for a deleted Scope,
                // recorded as a deletion instant and no path.
                ScopeObservation::Deleted => ScopeHistoryValue::Tombstone { deleted_at: recorded_at },
            };
            let mut entry = record(
                Grain::scope(),
                ValueColumns::of_scope(&value),
                MemberColumns::of(HistoryMembers::default())?,
                // Clause 4.5.6: the temporal Scope carries `observedAt`,
                // `createdAt` and `modifiedAt`; a Scope appended by this write
                // is created and last modified at the write.
                SemanticTimeAxes {
                    observed_at: *observed_at,
                    created_at: Some(recorded_at),
                    modified_at: Some(recorded_at),
                },
                instance,
            );
            if let Some(stored) = value.path().map(StoredScopePath::from) {
                entry.verbatim_scope_path = Some(String::from(stored.written));
                entry.scope_path = Some(stored.label_path);
            }
            Ok(entry)
        }
    }
}

impl TemporalEntryRecord {
    /// The fields that identify the entry, every value and member NULL: its
    /// kind and value code, its attribute and `datasetId`, and its instance
    /// identifiers.
    fn identity(&self) -> TemporalEntryRecord {
        TemporalEntryRecord {
            entry_kind: self.entry_kind,
            attribute_name_key: self.attribute_name_key,
            // clone: the identity owns what it binds; a `datasetId` and a client `instanceId` are rare and short.
            dataset_id: self.dataset_id.clone(),
            attribute_kind: self.attribute_kind,
            value_kind: self.value_kind,
            observed_at: None,
            created_at: None,
            modified_at: None,
            deleted_at: None,
            value_date_time: None,
            expires_at: None,
            generated_instance_id: self.generated_instance_id,
            value_type_key: None,
            unit_code_id: None,
            value_boolean: None,
            verbatim_instance_id: self.verbatim_instance_id.clone(),
            value_number: None,
            value_string: None,
            value_geometry: None,
            value_geometry_members: None,
            value_object: None,
            value_uri: None,
            object_type_keys: None,
            proof_seals: None,
            sub_attributes: None,
            scope_path: None,
            verbatim_scope_path: None,
        }
    }

    /// The entry an observation a Context Source answered is staged as for
    /// an aggregation, which reads the value and its unit alone (clause
    /// 4.5.19): the rest of its metadata names what this tenant may never
    /// have keyed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError`] when the attribute name has no key in
    /// `keys`, or the value cannot be stored.
    pub(crate) fn answered(row: &TemporalHistoryRow, keys: &NameKeys) -> Result<TemporalEntryRecord, TemporalStoreError> {
        observation_record(row, HistoryMembers::unit_alone(row.metadata.as_ref()), keys)
    }
}

/// The entry `row` is recorded as, beside the `members` its metadata stores
/// as.
fn observation_record(row: &TemporalHistoryRow, members: HistoryMembers, keys: &NameKeys) -> Result<TemporalEntryRecord, TemporalStoreError> {
    Ok(record(
        Grain::attribute(
            TemporalEntryKind::Observation,
            &row.attribute_name,
            row.dataset_id.as_ref(),
            stored_attribute_kind(row.attribute_type),
            keys,
        )?,
        ValueColumns::of_attribute(AttributeHistoryValue::of_observation(&row.value)?)?,
        MemberColumns::of(members)?,
        // Each axis is kept exactly as the observation carried it: one that
        // declared none matches no window on that Temporal Property (clauses
        // 4.11 and 5.2.21).
        SemanticTimeAxes {
            observed_at: row.observed_at,
            created_at: row.created_at,
            modified_at: row.modified_at,
        },
        &row.instance,
    ))
}

/// The field groups an entry is assembled from.
mod entry_fields;
#[cfg(test)]
pub(crate) mod tests;
