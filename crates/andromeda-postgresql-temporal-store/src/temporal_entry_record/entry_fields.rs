//! The three groups of fields one queued entry is assembled from: the grain
//! it extends, its value and its metadata.

use crate::{
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    history_members::HistoryMembers,
    scope_history_value::ScopeHistoryValue,
    semantic_time_axes::SemanticTimeAxes,
    stored_instance_columns::{generated_instance_id, verbatim_instance_id},
    temporal_entry_record::TemporalEntryRecord,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_postgresql_attribute_instance::{
    name_keys::NameKeys,
    stored_json::StoredJson,
    sub_attribute::{EwkbGeometry, SubAttributeRecord},
};
use andromeda_postgresql_codes::{
    attribute_kind::AttributeKind,
    stored_unit_code::stored_unit_code,
    temporal_entry_kind::TemporalEntryKind,
    value_kind::ValueKind,
};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_temporal::scope_entry_kind::ScopeEntryKind;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::types::Json;
use std::borrow::Cow;

/// The fields that name the stream an entry extends, or none for a Scope.
pub(super) struct Grain {
    /// `entry_kind`.
    entry_kind: TemporalEntryKind,
    /// `attribute_name_key`.
    attribute_name_key: Option<i32>,
    /// `dataset_id`.
    dataset_id: Option<String>,
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
            // clone: the invariant owns the name the dictionary failed to resolve.
            .ok_or_else(|| TemporalStoreInvariant::AttributeNameUnresolved(attribute_name.clone()))?;
        Ok(Grain {
            entry_kind: kind,
            attribute_name_key: Some(attribute_name_key.get()),
            dataset_id: dataset_id.map(|dataset| String::from(dataset.as_str())),
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
            dataset_id: None,
            attribute_kind: None,
        }
    }
}

/// The value fields of one entry: exactly one typed field populated, the one
/// `value_kind` names, or none and `deleted_at` for the end of an instance.
#[derive(Default)]
pub(super) struct ValueColumns {
    /// `value_kind`.
    value_kind: i16,
    /// `deleted_at`.
    deleted_at: Option<DateTime<Utc>>,
    /// `value_date_time`.
    value_date_time: Option<DateTime<Utc>>,
    /// `value_boolean`.
    value_boolean: Option<bool>,
    /// `value_number`.
    value_number: Option<BigDecimal>,
    /// `value_string`.
    value_string: Option<String>,
    /// `value_language`.
    value_language: Option<String>,
    /// `value_datatype_key`.
    value_datatype_key: Option<i32>,
    /// `value_geometry`.
    value_geometry: Option<EwkbGeometry>,
    /// `value_geometry_reach`.
    value_geometry_reach: Option<EwkbGeometry>,
    /// `value_geometry_members`.
    value_geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    value_object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    value_uri: Option<String>,
}

impl ValueColumns {
    /// The fields an attribute value is stored in.
    pub(super) fn of_attribute(value: &AttributeHistoryValue) -> Result<ValueColumns, TemporalStoreError> {
        Ok(ValueColumns {
            value_kind: ValueKind::code(value.kind()),
            deleted_at: value.deleted_at(),
            value_date_time: value.date_time(),
            value_boolean: value.boolean(),
            // clone: the field owns the decimal it binds, and the value gives up none.
            value_number: value.number().cloned(),
            value_string: value.string().map(Cow::into_owned),
            value_language: value.language().map(|language| language.as_str().to_owned()),
            value_datatype_key: value.datatype_key(),
            value_geometry: value.geometry().map(EwkbGeometry::of).transpose()?,
            value_geometry_reach: value.geometry().and_then(EwkbGeometry::reach_of),
            value_geometry_members: value.geometry_members().map(|members| Json(members.to_owned())),
            value_object: value.object().map(|object| Json(object.to_owned())),
            value_uri: value.uri().map(|uri| String::from(uri.as_str())),
        })
    }

    /// The fields a Scope entry's value is stored in: its `scope_history`
    /// code and, for a deletion, its instant. The path travels in the Scope
    /// fields.
    pub(super) fn of_scope(value: &ScopeHistoryValue) -> ValueColumns {
        ValueColumns {
            value_kind: value.kind().code(),
            deleted_at: value.deleted_at(),
            ..ValueColumns::default()
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
    /// `proof_seals`.
    proof_seals: Option<Json<Box<RawValue>>>,
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
            proof_seals,
            sub_attributes,
        } = members;
        let sub_attributes = sub_attributes.into_iter().map(SubAttributeRecord::try_from).collect::<Result<Vec<_>, _>>()?;
        Ok(MemberColumns {
            value_type_key: value_type_key.map(EntityTypeNameKey::get),
            object_type_keys: (!object_type_keys.is_empty()).then(|| object_type_keys.into_iter().map(EntityTypeNameKey::get).collect()),
            unit_code_id: unit_code.map(stored_unit_code),
            expires_at,
            proof_seals: proof_seals.map(StoredJson::into_json),
            sub_attributes: (!sub_attributes.is_empty()).then_some(sub_attributes),
        })
    }
}

/// Assemble one attribute entry from its parts and the instance it records.
pub(super) fn attribute_record(
    grain: Grain,
    value: ValueColumns,
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
pub(super) fn record(grain: Grain, value: ValueColumns, members: MemberColumns, axes: SemanticTimeAxes) -> TemporalEntryRecord {
    let Grain {
        entry_kind,
        attribute_name_key,
        dataset_id,
        attribute_kind,
    } = grain;
    let ValueColumns {
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
        proof_seals,
        sub_attributes,
    } = members;
    TemporalEntryRecord {
        entry_kind: entry_kind.code(),
        attribute_name_key,
        dataset_id,
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
        proof_seals,
        sub_attributes,
        scope_path: None,
        verbatim_scope_path: None,
    }
}
