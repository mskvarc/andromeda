//! One top-level attribute instance as the `attribute_instance_record`
//! composite: a `current_attributes` row without its Entity, field for field.
//!
//! The record keeps the row's length split: a String or a URI past its B-tree
//! bound sits in its overflow field, so SQL moves a record into a row field
//! for field.

use crate::current_attribute::row::{CurrentAttributeRow, row_column};
use andromeda_postgresql_attribute_instance::{
    stored_members::{InstanceMembers, MemberColumns, StoredInstanceError},
    sub_attribute::{EwkbGeometry, SubAttributeElement, SubAttributeRecord},
};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_schema::current::CurrentAttributes;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::types::Json;

/// The `attribute_instance_record` composite, field for field in declaration
/// order.
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "attribute_instance_record")]
pub struct AttributeInstanceRecord {
    /// `created_at`.
    pub created_at: DateTime<Utc>,
    /// `modified_at`.
    pub modified_at: DateTime<Utc>,
    /// `attribute_name_key`.
    pub attribute_name_key: i32,
    /// `ordinal`.
    pub ordinal: i32,
    /// `attribute_kind`.
    pub attribute_kind: i16,
    /// `value_kind`.
    pub value_kind: i16,
    /// `value_date_time`.
    pub value_date_time: Option<DateTime<Utc>>,
    /// `observed_at`.
    pub observed_at: Option<DateTime<Utc>>,
    /// `expires_at`.
    pub expires_at: Option<DateTime<Utc>>,
    /// `value_datatype_key`.
    pub value_datatype_key: Option<i32>,
    /// `value_type_key`.
    pub value_type_key: Option<i32>,
    /// `unit_code_id`.
    pub unit_code_id: Option<i16>,
    /// `value_boolean`.
    pub value_boolean: Option<bool>,
    /// `dataset_id`.
    pub dataset_id: Option<String>,
    /// `value_number`.
    pub value_number: Option<BigDecimal>,
    /// `value_string`, a String within the B-tree bound.
    pub value_string: Option<String>,
    /// `value_language`.
    pub value_language: Option<String>,
    /// `value_geometry`.
    pub value_geometry: Option<EwkbGeometry>,
    /// `value_geometry_members`.
    pub value_geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    pub value_object: Option<Json<Box<RawValue>>>,
    /// `value_uri`, a URI within the B-tree bound.
    pub value_uri: Option<String>,
    /// `value_long_text`, a String past the B-tree bound.
    pub value_long_text: Option<String>,
    /// `value_long_uri`, a URI past the B-tree bound.
    pub value_long_uri: Option<String>,
    /// `object_type_keys`, NULL when the list is empty.
    pub object_type_keys: Option<Vec<i32>>,
    /// `proof_seals`.
    pub proof_seals: Option<Json<Box<RawValue>>>,
    /// `sub_attributes`, NULL when the instance has none.
    pub sub_attributes: Option<Vec<SubAttributeRecord>>,
}

impl TryFrom<CurrentAttributeRow> for AttributeInstanceRecord {
    type Error = EwkbError;

    fn try_from(row: CurrentAttributeRow) -> Result<AttributeInstanceRecord, EwkbError> {
        let CurrentAttributeRow {
            ordinal,
            members,
            created_at,
            modified_at,
            sub_attributes,
        } = row;
        let column = row_column(&members.value);
        let MemberColumns {
            attribute_name_key,
            dataset_id,
            attribute_kind,
            value_kind,
            value_number,
            value_string,
            value_boolean,
            value_date_time,
            value_geometry,
            value_geometry_members,
            value_object,
            value_uri,
            value_datatype_key,
            value_language,
            value_type_key,
            object_type_keys,
            proof_seals,
            unit_code_id,
            observed_at,
            expires_at,
        } = MemberColumns::try_from(members)?;
        let (value_string, value_long_text) = if column == CurrentAttributes::ValueLongText {
            (None, value_string)
        } else {
            (value_string, None)
        };
        let (value_uri, value_long_uri) = if column == CurrentAttributes::ValueLongUri {
            (None, value_uri)
        } else {
            (value_uri, None)
        };
        let sub_attributes = if sub_attributes.is_empty() {
            None
        } else {
            Some(sub_attributes.into_iter().map(SubAttributeRecord::try_from).collect::<Result<_, _>>()?)
        };
        Ok(AttributeInstanceRecord {
            created_at,
            modified_at,
            attribute_name_key,
            ordinal,
            attribute_kind,
            value_kind,
            value_date_time,
            observed_at,
            expires_at,
            value_datatype_key,
            value_type_key,
            unit_code_id,
            value_boolean,
            dataset_id,
            value_number,
            value_string,
            value_language,
            value_geometry,
            value_geometry_members,
            value_object,
            value_uri,
            value_long_text,
            value_long_uri,
            object_type_keys,
            proof_seals,
            sub_attributes,
        })
    }
}

impl TryFrom<AttributeInstanceRecord> for CurrentAttributeRow {
    type Error = StoredInstanceError;

    fn try_from(record: AttributeInstanceRecord) -> Result<CurrentAttributeRow, StoredInstanceError> {
        let AttributeInstanceRecord {
            created_at,
            modified_at,
            attribute_name_key,
            ordinal,
            attribute_kind,
            value_kind,
            value_date_time,
            observed_at,
            expires_at,
            value_datatype_key,
            value_type_key,
            unit_code_id,
            value_boolean,
            dataset_id,
            value_number,
            value_string,
            value_language,
            value_geometry,
            value_geometry_members,
            value_object,
            value_uri,
            value_long_text,
            value_long_uri,
            object_type_keys,
            proof_seals,
            sub_attributes,
        } = record;
        let columns = MemberColumns {
            attribute_name_key,
            dataset_id,
            attribute_kind,
            value_kind,
            value_number,
            value_string: one_side(value_string, value_long_text, value_kind)?,
            value_boolean,
            value_date_time,
            value_geometry,
            value_geometry_members,
            value_object,
            value_uri: one_side(value_uri, value_long_uri, value_kind)?,
            value_datatype_key,
            value_language,
            value_type_key,
            object_type_keys,
            proof_seals,
            unit_code_id,
            observed_at,
            expires_at,
        };
        Ok(CurrentAttributeRow {
            ordinal,
            members: InstanceMembers::try_from(columns)?,
            created_at,
            modified_at,
            sub_attributes: sub_attributes
                .unwrap_or_default()
                .into_iter()
                .map(SubAttributeElement::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

/// The one String or URI a pair of length-split fields holds, on whichever
/// side of the split it sits.
///
/// # Errors
///
/// Returns [`StoredInstanceError`] when both sides hold a value, or when the
/// value kind is a code no release defines.
fn one_side(within: Option<String>, past: Option<String>, value_kind: i16) -> Result<Option<String>, StoredInstanceError> {
    match (within, past) {
        (Some(_), Some(_)) => Err(StoredInstanceError::ValueFields {
            kind: ValueKind::try_from(value_kind)?,
        }),
        (within, past) => Ok(within.or(past)),
    }
}
