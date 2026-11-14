//! The three groups of fields one queued entry is assembled from: the grain
//! it extends, its value and its metadata.

use crate::{
    error::TemporalStoreError,
    history_members::HistoryMembers,
    history_value_columns::HistoryValueColumns,
    semantic_time_axes::SemanticTimeAxes,
    stored_instance_columns::{generated_instance_id, verbatim_instance_id},
    temporal_entry_record::TemporalEntryRecord,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_postgresql_attribute_instance::{name_keys::NameKeys, sub_attribute::SubAttributeRecord};
use andromeda_postgresql_stored_values::{
    attribute_kind::AttributeKind,
    dataset_key::DatasetKey,
    entity_type_name_key::EntityTypeNameKey,
    stored_unit_code::stored_unit_code,
    temporal_entry_kind::TemporalEntryKind,
};
use andromeda_temporal::scope_entry_kind::ScopeEntryKind;
use chrono::{DateTime, Utc};

/// The fields that name the stream an entry extends, or none for a Scope.
pub(super) struct Grain {
    /// `entry_kind`.
    entry_kind: TemporalEntryKind,
    /// `attribute_name_key`.
    attribute_name_key: Option<i32>,
    /// `dataset_key`, NULL for a Scope.
    dataset_key: Option<String>,
    /// `attribute_kind`.
    attribute_kind: Option<AttributeKind>,
}

impl Grain {
    /// The grain of an attribute entry of `kind`, recorded in the attribute
    /// family `attribute_kind`.
    pub(super) fn attribute(
        kind: TemporalEntryKind,
        attribute_name: &AttributeName,
        dataset_id: Option<&Identifier>,
        attribute_kind: AttributeKind,
        keys: &NameKeys,
    ) -> Result<Grain, TemporalStoreError> {
        let attribute_name_key = keys
            .attribute(attribute_name)
            // clone: the error outlives the borrowed attribute name it failed on.
            .ok_or_else(|| TemporalStoreInvariant::AttributeNameUnresolved(attribute_name.clone()))?;
        Ok(Grain {
            entry_kind: kind,
            attribute_name_key: Some(attribute_name_key.get()),
            dataset_key: Some(DatasetKey::of(dataset_id).as_str().to_owned()),
            attribute_kind: Some(attribute_kind),
        })
    }

    /// The grain of a Scope entry of `kind`, which names no stream (clause
    /// 4.18 gives a Scope no dataset and no attribute family).
    pub(super) const fn scope(kind: ScopeEntryKind) -> Grain {
        Grain {
            entry_kind: match kind {
                ScopeEntryKind::Event => TemporalEntryKind::Scope,
                ScopeEntryKind::Assertion => TemporalEntryKind::ScopeAssertion,
            },
            attribute_name_key: None,
            dataset_key: None,
            attribute_kind: None,
        }
    }
}

/// The metadata fields of one entry, every name a key.
pub(super) struct MemberColumns {
    /// `value_type_key`.
    value_type_key: Option<i32>,
    /// `object_type_keys`.
    object_type_keys: Option<Vec<i32>>,
    /// `unit_code_id`.
    unit_code_id: Option<i16>,
    /// `expires_at`.
    expires_at: Option<DateTime<Utc>>,
    /// `sub_attributes`.
    sub_attributes: Option<Vec<SubAttributeRecord>>,
}

impl MemberColumns {
    /// The fields `members` is stored in, an empty list as NULL.
    pub(super) fn of(members: HistoryMembers) -> Result<MemberColumns, TemporalStoreError> {
        let HistoryMembers {
            value_type_key,
            object_type_keys,
            unit_code,
            expires_at,
            sub_attributes,
        } = members;
        let sub_attributes = sub_attributes.into_iter().map(SubAttributeRecord::try_from).collect::<Result<Vec<_>, _>>()?;
        Ok(MemberColumns {
            value_type_key: value_type_key.map(EntityTypeNameKey::get),
            object_type_keys: (!object_type_keys.is_empty()).then(|| object_type_keys.into_iter().map(EntityTypeNameKey::get).collect()),
            unit_code_id: unit_code.map(stored_unit_code),
            expires_at,
            sub_attributes: (!sub_attributes.is_empty()).then_some(sub_attributes),
        })
    }
}

/// Assemble one attribute entry from its parts and the instance it records.
pub(super) fn attribute_record(
    grain: Grain,
    value: HistoryValueColumns,
    members: MemberColumns,
    axes: SemanticTimeAxes,
    instance: &InstanceIdentity,
) -> TemporalEntryRecord {
    TemporalEntryRecord {
        generated_instance_id: generated_instance_id(instance),
        verbatim_instance_id: verbatim_instance_id(instance).map(String::from),
        ..record(grain, value, members, axes)
    }
}

/// Assemble one entry from its parts, with no instance identity: an instance
/// of the Scope has none (clause 4.5.6).
pub(super) fn record(grain: Grain, value: HistoryValueColumns, members: MemberColumns, axes: SemanticTimeAxes) -> TemporalEntryRecord {
    let Grain {
        entry_kind,
        attribute_name_key,
        dataset_key,
        attribute_kind,
    } = grain;
    let HistoryValueColumns {
        value_kind,
        deleted_at,
        value_date_time,
        value_boolean,
        value_number,
        value_string,
        value_language,
        value_datatype_key,
        value_geometry,
        value_geometry_reach,
        value_geometry_members,
        value_object,
        value_uri,
    } = value;
    let MemberColumns {
        value_type_key,
        object_type_keys,
        unit_code_id,
        expires_at,
        sub_attributes,
    } = members;
    TemporalEntryRecord {
        entry_kind: entry_kind.code(),
        attribute_name_key,
        dataset_key,
        attribute_kind: attribute_kind.map(AttributeKind::code),
        value_kind,
        observed_at: axes.observed_at,
        created_at: axes.created_at,
        modified_at: axes.modified_at,
        deleted_at,
        value_date_time,
        expires_at,
        generated_instance_id: None,
        value_type_key,
        value_datatype_key,
        unit_code_id,
        value_boolean,
        verbatim_instance_id: None,
        value_number,
        value_string,
        value_language,
        value_geometry,
        value_geometry_reach,
        value_geometry_members,
        value_object,
        value_uri,
        object_type_keys,
        sub_attributes,
        scope_path: None,
        verbatim_scope_path: None,
    }
}
